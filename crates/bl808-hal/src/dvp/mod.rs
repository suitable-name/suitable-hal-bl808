//! Digital Video Port (DVP) Driver
//!
//! DMA-backed frame capture with interrupt signalling.
//!
//! ## Register map (relative to DVP base)
//!
//! | Offset | Name                  | Access | Description                                     |
//! |--------|-----------------------|--------|-------------------------------------------------|
//! | 0x00   | dvp2axi_configue      | R/W    | Enable, polarity, burst, data mode              |
//! | 0x04   | dvp2axi_addr_start    | R/W    | DMA ring-buffer base address (write-path)       |
//! | 0x08   | dvp2axi_mem_bcnt      | R/W    | Ring-buffer size in bursts                      |
//! | 0x0c   | dvp_status_and_error  | R/W    | Interrupt enables (bits 8-11), status (r/o)     |
//! | 0x10   | dvp2axi_frame_bcnt    | R/W    | Single frame byte count                         |
//! | 0x14   | dvp_frame_fifo_pop    | W      | FIFO pop (bit 0) + interrupt clears (bits 4-9)  |
//! | 0x30   | dvp2axi_hsync_crop    | R/W    | HSYNC active window: start[31:16], end[15:0]    |
//! | 0x34   | dvp2axi_vsync_crop    | R/W    | VSYNC active window: start[31:16], end[15:0]    |
//! | 0x40   | frame_start_addr0     | R      | Status: address of oldest buffered frame        |
//! | 0x48   | frame_start_addr1     | R      | Status: address of 2nd buffered frame           |
//! | 0x60   | frame_id_sts01        | R      | Frame IDs for buffers 0 and 1                   |
//!
//! ## Single-buffer vs. double-buffer model
//!
//! The DVP DMA engine exposes a **single writable base address** (`dvp2axi_addr_start`).
//! The hardware maintains an internal FIFO of completed frame addresses: after each frame the
//! destination address is latched into `frame_start_addr0`, `frame_start_addr1`, … (read-only
//! status registers at 0x40/0x48/0x50/0x58).  The number of frames currently held in the FIFO is
//! `frame_valid_cnt` in `dvp_status_and_error`.
//!
//! True double-buffering (ping-pong) is achieved by:
//! 1. Setting the ring-buffer base (`dvp2axi_addr_start`) to `addr0` and the ring size
//!    (`dvp2axi_mem_bcnt`) to cover **two** frame-worth of burst counts.
//! 2. Setting `dvp2axi_frame_bcnt` to one frame's byte count.
//! 3. The hardware fills `addr0`, latches it into the FIFO, then fills `addr0 + frame_size`,
//!    latches that, and wraps back to `addr0`.
//! 4. After each frame interrupt, read `frame_start_addr0` to learn which physical buffer was
//!    just completed, pop the FIFO entry, then compare the address to the two programmed
//!    addresses to determine which index (0 or 1) the CPU should process.
//!
//! The frame-complete interrupt status bit (`sts_frame_int`, bit 14) is read-only.
//! Clearing the interrupt is done by writing `reg_int_frame_clr` (bit 6) in
//! `dvp_frame_fifo_pop`.

use bl808_pac::Dvp0;

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_DVP0_PTR: core::cell::Cell<*const bl808_pac::dvp0::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    ActiveHigh,
    ActiveLow,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub hsync_polarity: Polarity,
    pub vsync_polarity: Polarity,
    pub pclk_polarity: Polarity,
    pub width: u16,
    pub height: u16,
}

pub struct Dvp {
    _dvp: Dvp0,
    /// Shadow copies of the two double-buffer addresses, used to identify which
    /// buffer index just completed when reading `frame_start_addr0`.
    db_addr: [u32; 2],
}

impl Dvp {
    pub fn new(dvp: Dvp0) -> Self {
        Self {
            _dvp: dvp,
            db_addr: [0; 2],
        }
    }

    fn regs(&self) -> &bl808_pac::dvp0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return unsafe { &*Dvp0::PTR };
        #[cfg(any(test, feature = "mock"))]
        return unsafe { &*MOCK_DVP0_PTR.with(|p| p.get()) };
    }

    /// Initialise the DVP controller and disable it first.
    ///
    /// Programs HSYNC/VSYNC polarity and the active-window crop registers so that
    /// the hardware samples exactly `config.width × config.height` pixels per frame.
    ///
    /// The crop registers (`dvp2axi_hsync_crop` at 0x30, `dvp2axi_vsync_crop` at 0x34)
    /// are now accessed via real PAC fields — the previous raw-pointer hack has been removed.
    pub fn init(&mut self, config: Config) {
        self.disable();

        let regs = self.regs();
        regs.dvp2axi_configue().modify(|_, w| {
            w.reg_fram_vld_pol()
                .bit(config.vsync_polarity == Polarity::ActiveHigh)
                .reg_line_vld_pol()
                .bit(config.hsync_polarity == Polarity::ActiveHigh)
        });

        // Program the HSYNC active window: sample pixels from 0 to width-1.
        // reg_hsync_act_start[31:16] = 0 (start at pixel 0),
        // reg_hsync_act_end[15:0]    = width (exclusive end).
        unsafe {
            regs.dvp2axi_hsync_crop().write(|w| {
                w.reg_hsync_act_start()
                    .bits(0)
                    .reg_hsync_act_end()
                    .bits(config.width)
            });

            // Program the VSYNC active window: sample lines from 0 to height-1.
            regs.dvp2axi_vsync_crop().write(|w| {
                w.reg_vsync_act_start()
                    .bits(0)
                    .reg_vsync_act_end()
                    .bits(config.height)
            });
        }
    }

    /// Enable the DVP receiver.
    pub fn enable(&mut self) {
        self.regs()
            .dvp2axi_configue()
            .modify(|_, w| w.reg_dvp_enable().set_bit());
    }

    /// Disable the DVP receiver.
    pub fn disable(&mut self) {
        self.regs()
            .dvp2axi_configue()
            .modify(|_, w| w.reg_dvp_enable().clear_bit());
    }

    /// Set the DMA destination address for incoming frame data (single-buffer mode).
    ///
    /// # Safety
    ///
    /// `addr` must point to a valid memory region large enough for one full
    /// frame (`width × height × bytes_per_pixel`) and must remain valid for
    /// the duration of the capture.
    pub unsafe fn set_frame_buffer(&mut self, addr: u32) {
        self.regs()
            .dvp2axi_addr_start()
            .write(|w| w.reg_addr_start().bits(addr));
    }

    /// Configure two frame buffers for double-buffered (ping-pong) capture.
    ///
    /// The DVP DMA engine uses a single base address and a ring-buffer of
    /// `dvp2axi_mem_bcnt` bursts.  By pointing the ring at `addr0` and sizing
    /// it to hold exactly two frames the hardware alternates between `addr0`
    /// (even frames) and `addr1` (odd frames) autonomously.
    ///
    /// After each frame the hardware latches the completed buffer address into
    /// `FRAME_START_ADDR0` (read-only status, 0x40).  Call [`pop_frame`] to
    /// advance the hardware FIFO after reading that buffer.
    ///
    /// # Arguments
    ///
    /// * `addr0` — address of the first (even) frame buffer.
    /// * `addr1` — address of the second (odd) frame buffer.
    ///   Must equal `addr0 + frame_size` (caller's responsibility).
    /// * `frame_bytes` — byte size of one complete frame.  Written to
    ///   `dvp2axi_frame_bcnt` and used to size the ring.
    /// * `burst_bytes` — AXI burst size in bytes (e.g. 128 for INCR16 × 4 bytes).
    ///   Must divide evenly into `frame_bytes`.
    ///
    /// # Safety
    ///
    /// Both `addr0` and `addr1` must point to valid, distinct, non-overlapping
    /// memory regions each of size `frame_bytes`, and both must remain valid for
    /// the duration of all captures.
    pub unsafe fn set_frame_buffers(
        &mut self,
        addr0: u32,
        addr1: u32,
        frame_bytes: u32,
        burst_bytes: u32,
    ) {
        self.db_addr = [addr0, addr1];

        let regs = self.regs();

        // Base of the DMA ring = addr0.
        regs.dvp2axi_addr_start()
            .write(|w| w.reg_addr_start().bits(addr0));

        // Frame byte count: the hardware advances by this amount each frame.
        regs.dvp2axi_frame_bcnt()
            .write(|w| w.reg_frame_byte_cnt().bits(frame_bytes));

        // Ring-buffer burst count: two frames worth of bursts.
        let ring_bursts = (frame_bytes * 2) / burst_bytes;
        regs.dvp2axi_mem_bcnt()
            .write(|w| w.reg_mem_burst_cnt().bits(ring_bursts));
    }

    /// Return the address of the oldest completed frame currently in the hardware FIFO.
    ///
    /// This is the address reported by the `FRAME_START_ADDR0` status register (0x40).
    /// Valid only when `frame_valid_cnt > 0` (i.e. at least one frame has been captured).
    pub fn current_frame_addr(&self) -> u32 {
        self.regs()
            .frame_start_addr0()
            .read()
            .frame_start_addr_0()
            .bits()
    }

    /// Return the number of completed frames waiting in the hardware FIFO.
    ///
    /// Reads `frame_valid_cnt` from `dvp_status_and_error`.
    pub fn frame_valid_count(&self) -> u8 {
        self.regs()
            .dvp_status_and_error()
            .read()
            .frame_valid_cnt()
            .bits()
    }

    /// Identify which double-buffer index (0 or 1) corresponds to `addr`.
    ///
    /// Returns `Some(0)` if `addr == db_addr[0]`, `Some(1)` if `addr == db_addr[1]`,
    /// or `None` if the address does not match either configured buffer
    /// (e.g. `set_frame_buffers` was never called).
    pub fn buffer_index_for_addr(&self, addr: u32) -> Option<u8> {
        if addr == self.db_addr[0] {
            Some(0)
        } else if addr == self.db_addr[1] {
            Some(1)
        } else {
            None
        }
    }

    /// Pop the oldest completed frame from the hardware FIFO.
    ///
    /// Writes `rfifo_pop` (bit 0) in `dvp_frame_fifo_pop`.  The hardware then
    /// exposes the next buffered frame (if any) via `frame_start_addr0`.
    pub fn pop_frame(&mut self) {
        self.regs()
            .dvp_frame_fifo_pop()
            .write(|w| w.rfifo_pop().set_bit());
    }

    /// Enable the frame-complete interrupt.
    pub fn enable_frame_interrupt(&mut self) {
        self.regs()
            .dvp_status_and_error()
            .modify(|_, w| w.reg_int_frame_en().set_bit());
    }

    /// Disable the frame-complete interrupt.
    pub fn disable_frame_interrupt(&mut self) {
        self.regs()
            .dvp_status_and_error()
            .modify(|_, w| w.reg_int_frame_en().clear_bit());
    }

    /// Return `true` if the frame-complete interrupt is pending.
    pub fn is_frame_done(&self) -> bool {
        self.regs()
            .dvp_status_and_error()
            .read()
            .sts_frame_int()
            .bit_is_set()
    }

    /// Clear the frame-complete interrupt flag.
    ///
    /// Write `reg_int_frame_clr` (bit 6) in `dvp_frame_fifo_pop`.
    /// Note: `sts_frame_int` is read-only; this is the only way to clear it.
    pub fn clear_frame_interrupt(&mut self) {
        self.regs()
            .dvp_frame_fifo_pop()
            .write(|w| w.reg_int_frame_clr().set_bit());
    }

    /// Blocking single-buffer capture: wait until a full frame has been written to the DMA
    /// buffer, then clear the interrupt flag and return.
    ///
    /// Call [`set_frame_buffer`] and [`enable`] before calling this.
    pub fn capture_frame_blocking(&mut self) {
        self.enable_frame_interrupt();
        while !self.is_frame_done() {
            core::hint::spin_loop();
        }
        self.clear_frame_interrupt();
        self.disable_frame_interrupt();
    }

    /// Blocking double-buffered capture.
    ///
    /// Waits for the next frame-complete interrupt, clears it, then determines
    /// which buffer index (0 or 1) the CPU can now safely read.
    ///
    /// ## Double-buffer handoff protocol
    ///
    /// The hardware alternates DMA writes between `addr0` and `addr1` (set up
    /// via [`set_frame_buffers`]).  On each frame-complete interrupt:
    ///
    /// 1. `FRAME_START_ADDR0` (0x40) holds the address of the **just-completed**
    ///    buffer — this is the one the CPU should now read.
    /// 2. The DVP immediately begins filling the **other** buffer, so the CPU
    ///    has until the next frame to finish reading the completed one.
    /// 3. This function pops the completed entry from the hardware FIFO via
    ///    `rfifo_pop`, so the next call will see the subsequent frame.
    ///
    /// Returns the buffer index (0 or 1) that is now safe to read, i.e. the one
    /// the hardware just finished writing.  Returns `None` if the completed
    /// address does not match either configured buffer (caller misconfiguration).
    ///
    /// Call [`set_frame_buffers`] and [`enable`] before calling this.
    pub fn capture_frame_double_buffered(&mut self) -> Option<u8> {
        self.enable_frame_interrupt();
        while !self.is_frame_done() {
            core::hint::spin_loop();
        }
        self.clear_frame_interrupt();
        self.disable_frame_interrupt();

        // Read which physical buffer just completed before popping the FIFO.
        let completed_addr = self.current_frame_addr();

        // Advance the hardware FIFO so the next call sees the next frame.
        self.pop_frame();

        self.buffer_index_for_addr(completed_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::dvp0::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_DVP0_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_dvp_enable_sets_bit() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.enable();

        assert!(
            regs.dvp2axi_configue().read().reg_dvp_enable().bit_is_set(),
            "reg_dvp_enable should be set"
        );
    }

    #[test]
    fn test_dvp_disable_clears_bit() {
        let regs = setup();
        // Pre-fill with all ones
        unsafe { core::ptr::write_volatile(regs.dvp2axi_configue().as_ptr(), 0xFFFF_FFFFu32) };
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.disable();

        assert!(
            regs.dvp2axi_configue().read().reg_dvp_enable().bit_is_clear(),
            "reg_dvp_enable should be cleared"
        );
    }

    #[test]
    fn test_dvp_set_frame_buffer() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        unsafe { dvp.set_frame_buffer(0x8200_0000) };

        // dvp2axi_addr_start is readable; use raw pointer read
        let val = unsafe { core::ptr::read_volatile(regs.dvp2axi_addr_start().as_ptr()) };
        assert_eq!(val, 0x8200_0000);
    }

    #[test]
    fn test_dvp_frame_interrupt_enable() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.enable_frame_interrupt();

        assert!(
            regs.dvp_status_and_error().read().reg_int_frame_en().bit_is_set(),
            "reg_int_frame_en should be set"
        );
    }

    #[test]
    fn test_dvp_clear_frame_interrupt() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.clear_frame_interrupt();

        // dvp_frame_fifo_pop is write-only; read back via as_ptr()
        // reg_int_frame_clr = bit 6
        let val = unsafe { core::ptr::read_volatile(regs.dvp_frame_fifo_pop().as_ptr()) };
        assert_ne!(val & (1 << 6), 0, "reg_int_frame_clr should be written");
    }

    #[test]
    fn test_dvp_signal_polarity() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.init(Config {
            hsync_polarity: Polarity::ActiveLow,
            vsync_polarity: Polarity::ActiveHigh,
            pclk_polarity: Polarity::ActiveHigh,
            width: 640,
            height: 480,
        });

        let cfg = regs.dvp2axi_configue().read();
        // reg_fram_vld_pol = vsync polarity (ActiveHigh → set)
        assert!(cfg.reg_fram_vld_pol().bit_is_set(), "vsync polarity should be active-high");
        // reg_line_vld_pol = hsync polarity (ActiveLow → clear)
        assert!(cfg.reg_line_vld_pol().bit_is_clear(), "hsync polarity should be active-low");
    }

    #[test]
    fn test_dvp_init_programs_crop_registers() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.init(Config {
            hsync_polarity: Polarity::ActiveHigh,
            vsync_polarity: Polarity::ActiveHigh,
            pclk_polarity: Polarity::ActiveHigh,
            width: 320,
            height: 240,
        });

        // dvp2axi_hsync_crop: start=0 in bits[31:16], end=width in bits[15:0]
        let hcrop = regs.dvp2axi_hsync_crop().read();
        assert_eq!(hcrop.reg_hsync_act_start().bits(), 0, "hsync start should be 0");
        assert_eq!(hcrop.reg_hsync_act_end().bits(), 320, "hsync end should be width");

        // dvp2axi_vsync_crop: start=0 in bits[31:16], end=height in bits[15:0]
        let vcrop = regs.dvp2axi_vsync_crop().read();
        assert_eq!(vcrop.reg_vsync_act_start().bits(), 0, "vsync start should be 0");
        assert_eq!(vcrop.reg_vsync_act_end().bits(), 240, "vsync end should be height");
    }

    #[test]
    fn test_dvp_set_frame_buffers() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        const ADDR0: u32 = 0x8200_0000;
        const ADDR1: u32 = 0x8210_0000;
        const FRAME_BYTES: u32 = 640 * 480 * 2; // 16-bit pixels
        const BURST_BYTES: u32 = 128; // INCR16 × 4 bytes/beat

        unsafe { dvp.set_frame_buffers(ADDR0, ADDR1, FRAME_BYTES, BURST_BYTES) };

        // dvp2axi_addr_start should be set to addr0
        let addr_start = unsafe { core::ptr::read_volatile(regs.dvp2axi_addr_start().as_ptr()) };
        assert_eq!(addr_start, ADDR0, "addr_start should be addr0");

        // dvp2axi_frame_bcnt should be set to frame_bytes
        let frame_bcnt =
            unsafe { core::ptr::read_volatile(regs.dvp2axi_frame_bcnt().as_ptr()) };
        assert_eq!(frame_bcnt, FRAME_BYTES, "frame_bcnt should be frame_bytes");

        // dvp2axi_mem_bcnt should be (frame_bytes * 2) / burst_bytes
        let expected_mem_bcnt = (FRAME_BYTES * 2) / BURST_BYTES;
        let mem_bcnt = unsafe { core::ptr::read_volatile(regs.dvp2axi_mem_bcnt().as_ptr()) };
        assert_eq!(mem_bcnt, expected_mem_bcnt, "mem_bcnt should be two-frame ring");
    }

    #[test]
    fn test_dvp_buffer_index_for_addr() {
        let _regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        const ADDR0: u32 = 0x8200_0000;
        const ADDR1: u32 = 0x8210_0000;

        unsafe { dvp.set_frame_buffers(ADDR0, ADDR1, 0x10_0000, 128) };

        assert_eq!(dvp.buffer_index_for_addr(ADDR0), Some(0));
        assert_eq!(dvp.buffer_index_for_addr(ADDR1), Some(1));
        assert_eq!(dvp.buffer_index_for_addr(0xDEAD_BEEF), None);
    }

    #[test]
    fn test_dvp_pop_frame_writes_rfifo_pop() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let mut dvp = Dvp::new(dvp0);

        dvp.pop_frame();

        // rfifo_pop = bit 0 in dvp_frame_fifo_pop (write-only, read via raw ptr)
        let val = unsafe { core::ptr::read_volatile(regs.dvp_frame_fifo_pop().as_ptr()) };
        assert_ne!(val & 1, 0, "rfifo_pop (bit 0) should be written");
    }

    #[test]
    fn test_dvp_frame_valid_count() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let dvp = Dvp::new(dvp0);

        // Inject frame_valid_cnt = 3 into bits [20:16] of dvp_status_and_error
        // frame_valid_cnt is at bits 16..20 (5 bits)
        unsafe {
            core::ptr::write_volatile(
                regs.dvp_status_and_error().as_ptr(),
                3u32 << 16,
            )
        };

        assert_eq!(dvp.frame_valid_count(), 3, "frame_valid_count should read 3");
    }

    #[test]
    fn test_dvp_current_frame_addr() {
        let regs = setup();
        let dvp0 = unsafe { Dvp0::steal() };
        let dvp = Dvp::new(dvp0);

        // frame_start_addr0 is read-only at 0x40 in the register block.
        // Inject a value directly via raw pointer.
        unsafe {
            core::ptr::write_volatile(regs.frame_start_addr0().as_ptr(), 0x8210_0000u32)
        };

        assert_eq!(dvp.current_frame_addr(), 0x8210_0000);
    }
}
