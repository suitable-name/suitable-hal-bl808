//! ISO11898 (CAN) Driver
//!
//! SJA1000-compatible PeliCAN-mode driver for the BL808 CAN controller.
//!
//! In reset mode (mode.reset_request = 1):
//!   - bus_timing_0/1 are writable
//!   - TX/RX buffers are accessible for configuration
//!
//! In operational mode (reset_request = 0):
//!   - Frames can be transmitted via buffer registers
//!   - Received frames are read from the same buffer area
//!
//! Each `buffer(n)` is a cluster whose single byte-wide register is reached via
//! `.reg()` (PAC models the CAN data window as an array of 8-bit clusters).

use bl808_pac::Iso11898;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_CAN_PTR: core::cell::Cell<*const bl808_pac::iso11898::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanError {
    TxBufferBusy,
    RxEmpty,
    BusError,
}

/// A CAN frame (up to 8 data bytes, standard or extended ID).
#[derive(Debug, Clone, Copy)]
pub struct CanFrame {
    /// 11-bit standard ID or 29-bit extended ID.
    pub id: u32,
    /// True if this is an extended-format (29-bit) frame.
    pub extended: bool,
    /// True if this is a Remote Transmission Request.
    pub rtr: bool,
    /// Data length code (0–8).
    pub dlc: u8,
    /// Frame payload.
    pub data: [u8; 8],
}

impl CanFrame {
    pub fn new(id: u32, extended: bool, data: &[u8]) -> Self {
        let dlc = (data.len().min(8)) as u8;
        let mut buf = [0u8; 8];
        buf[..dlc as usize].copy_from_slice(&data[..dlc as usize]);
        Self { id, extended, rtr: false, dlc, data: buf }
    }

    pub fn remote(id: u32, extended: bool, dlc: u8) -> Self {
        Self { id, extended, rtr: true, dlc: dlc.min(8), data: [0u8; 8] }
    }
}

pub struct CanDriver<CAN> {
    _can: CAN,
}

impl<CAN: CanInstance> CanDriver<CAN> {
    pub fn new(can: CAN) -> Self {
        Self { _can: can }
    }

    fn regs(&self) -> &bl808_pac::iso11898::RegisterBlock {
        unsafe { &*CAN::ptr() }
    }

    /// Write one byte into CAN data/filter buffer slot `idx`.
    #[inline]
    fn write_buffer(&self, idx: usize, val: u8) {
        self.regs()
            .buffer(idx)
            .reg()
            .write(|w| unsafe { w.bits(val as u32) });
    }

    /// Read one byte from CAN data/filter buffer slot `idx`.
    #[inline]
    fn read_buffer(&self, idx: usize) -> u32 {
        self.regs().buffer(idx).reg().read().bits()
    }

    /// Enter or leave reset mode.
    ///
    /// Must be in reset mode to change baud rate.
    pub fn set_reset_mode(&mut self, reset: bool) {
        self.regs().mode().modify(|_, w| w.reset_request().bit(reset));
    }

    /// Configure CAN baud rate (call while in reset mode).
    ///
    /// - `brp`: Baud Rate Prescaler (0–63)
    /// - `sjw`: Synchronisation Jump Width (0–3)
    /// - `tseg1`: Time Segment 1 (0–15)
    /// - `tseg2`: Time Segment 2 (0–7)
    /// - `sam3`: Triple sampling (true = 3 samples)
    pub fn set_baud_rate(&mut self, brp: u8, sjw: u8, tseg1: u8, tseg2: u8, sam3: bool) {
        let regs = self.regs();
        regs.bus_timing_0().write(|w| unsafe {
            w.baud_rate_prescaler()
                .bits(brp & 0x3F)
                .sync_jump_width()
                .bits(sjw & 0x3)
        });
        regs.bus_timing_1().write(|w| unsafe {
            w.time_segment_1()
                .bits(tseg1 & 0xF)
                .time_segment_2()
                .bits(tseg2 & 0x7)
                .sampling()
                .bit(sam3)
        });
    }

    /// Transmit a CAN frame (non-blocking).
    ///
    /// Returns `Err(CanError::TxBufferBusy)` if the TX buffer is not available.
    pub fn transmit(&mut self, frame: &CanFrame) -> Result<(), CanError> {
        let regs = self.regs();

        // Check TX buffer status (bit 2 of status register)
        let sts = regs.status().read();
        if !sts.transmit_buffer_sts().bit_is_set() {
            return Err(CanError::TxBufferBusy);
        }

        // SJA1000 PeliCAN TX frame format:
        // buffer[0]: Frame Info (FF | RTR | DLC)
        // buffer[1..2]: ID bytes (standard: 2 bytes; extended: 4 bytes)
        // buffer[1+id_bytes..]: data bytes
        let ff_bit: u32 = if frame.extended { 1 << 7 } else { 0 };
        let rtr_bit: u32 = if frame.rtr { 1 << 6 } else { 0 };
        let frame_info = ff_bit | rtr_bit | (frame.dlc as u32 & 0xF);
        self.write_buffer(0, frame_info as u8);

        if frame.extended {
            // 29-bit extended ID packed into 4 bytes (bits 28..3 in first 4)
            let id29 = frame.id & 0x1FFF_FFFF;
            self.write_buffer(1, ((id29 >> 21) & 0xFF) as u8);
            self.write_buffer(2, ((id29 >> 13) & 0xFF) as u8);
            self.write_buffer(3, ((id29 >> 5) & 0xFF) as u8);
            self.write_buffer(4, ((id29 << 3) & 0xFF) as u8);
            for (i, &b) in frame.data[..frame.dlc as usize].iter().enumerate() {
                self.write_buffer(5 + i, b);
            }
        } else {
            // 11-bit standard ID packed into 2 bytes
            let id11 = frame.id & 0x7FF;
            self.write_buffer(1, ((id11 >> 3) & 0xFF) as u8);
            self.write_buffer(2, ((id11 << 5) & 0xFF) as u8);
            for (i, &b) in frame.data[..frame.dlc as usize].iter().enumerate() {
                self.write_buffer(3 + i, b);
            }
        }

        // Request transmission
        regs.command().write(|w| w.transmit_request().set_bit());
        Ok(())
    }

    /// Poll for a received frame (non-blocking).
    ///
    /// Returns `Err(CanError::RxEmpty)` when no frame is available.
    pub fn receive(&mut self) -> Result<CanFrame, CanError> {
        let regs = self.regs();

        let sts = regs.status().read();
        if sts.error_sts().bit_is_set() {
            return Err(CanError::BusError);
        }
        if !sts.receive_buffer_sts().bit_is_set() {
            return Err(CanError::RxEmpty);
        }

        let frame_info = self.read_buffer(0);
        let extended = (frame_info >> 7) & 1 != 0;
        let rtr = (frame_info >> 6) & 1 != 0;
        let dlc = (frame_info & 0xF) as u8;

        let (id, data_start) = if extended {
            let b1 = self.read_buffer(1);
            let b2 = self.read_buffer(2);
            let b3 = self.read_buffer(3);
            let b4 = self.read_buffer(4);
            let id = (b1 << 21) | (b2 << 13) | (b3 << 5) | (b4 >> 3);
            (id, 5usize)
        } else {
            let b1 = self.read_buffer(1);
            let b2 = self.read_buffer(2);
            let id = (b1 << 3) | (b2 >> 5);
            (id, 3usize)
        };

        let mut data = [0u8; 8];
        for (i, slot) in data.iter_mut().enumerate().take(dlc as usize) {
            *slot = self.read_buffer(data_start + i) as u8;
        }

        // Release the RX buffer back to hardware
        regs.command().write(|w| w.release_receive_buffer().set_bit());

        Ok(CanFrame { id, extended, rtr, dlc, data })
    }

    /// Return the raw status register value.
    pub fn get_status(&self) -> u32 {
        self.regs().status().read().bits()
    }
}

pub trait CanInstance: core::ops::Deref<Target = bl808_pac::iso11898::RegisterBlock> {
    fn ptr() -> *const bl808_pac::iso11898::RegisterBlock;
}

impl CanInstance for Iso11898 {
    fn ptr() -> *const bl808_pac::iso11898::RegisterBlock {
        #[cfg(not(test))]
        return Iso11898::PTR;
        #[cfg(test)]
        return MOCK_CAN_PTR.with(|p| p.get());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::iso11898::RegisterBlock;
    use std::boxed::Box;

    /// Leak a heap-allocated mock register block so the pointer handed to the
    /// driver stays valid for the whole test (the old by-value pattern returned
    /// a pointer into a freed stack frame).
    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 256] = Box::leak(Box::new([0u32; 256]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_CAN_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_can_reset_mode_toggle() {
        let regs = setup();
        let can = unsafe { Iso11898::steal() };
        let mut driver = CanDriver::new(can);

        driver.set_reset_mode(true);
        assert!(
            regs.mode().read().reset_request().bit_is_set(),
            "reset_request bit should be set"
        );

        driver.set_reset_mode(false);
        assert!(
            regs.mode().read().reset_request().bit_is_clear(),
            "reset_request bit should be cleared"
        );
    }

    #[test]
    fn test_can_set_baud_rate_writes_bt0_bt1() {
        let regs = setup();
        let can = unsafe { Iso11898::steal() };
        let mut driver = CanDriver::new(can);

        // brp=4, sjw=1, tseg1=13, tseg2=2, no triple sampling
        driver.set_baud_rate(4, 1, 13, 2, false);

        let bt0 = regs.bus_timing_0().read();
        assert_eq!(bt0.baud_rate_prescaler().bits(), 4, "BRP should be 4");
        assert_eq!(bt0.sync_jump_width().bits(), 1, "SJW should be 1");

        let bt1 = regs.bus_timing_1().read();
        assert_eq!(bt1.time_segment_1().bits(), 13, "TSEG1 should be 13");
        assert_eq!(bt1.time_segment_2().bits(), 2, "TSEG2 should be 2");
        assert!(bt1.sampling().bit_is_clear(), "sampling should be 0");
    }

    #[test]
    fn test_can_transmit_returns_busy_when_tx_not_ready() {
        let _regs = setup();
        // status: bit 2 = TBS (TX buffer status) — leave it 0 → busy
        let can = unsafe { Iso11898::steal() };
        let mut driver = CanDriver::new(can);

        let frame = CanFrame::new(0x123, false, &[1, 2, 3]);
        assert!(matches!(driver.transmit(&frame), Err(CanError::TxBufferBusy)));
    }

    #[test]
    fn test_can_transmit_standard_frame() {
        let regs = setup();

        // Set TBS (TX buffer status bit 2) so transmit proceeds. STATUS is
        // read-only in the PAC, so poke the mock directly via its address.
        unsafe { core::ptr::write_volatile(regs.status().as_ptr(), 1 << 2) };

        let can = unsafe { Iso11898::steal() };
        let mut driver = CanDriver::new(can);

        let frame = CanFrame::new(0x7FF, false, &[0xAA, 0xBB]);
        assert!(driver.transmit(&frame).is_ok());
    }

    #[test]
    fn test_can_receive_returns_empty_when_no_data() {
        let _regs = setup();
        let can = unsafe { Iso11898::steal() };
        let mut driver = CanDriver::new(can);

        // RBS (bit 0 of status) not set → empty
        assert!(matches!(driver.receive(), Err(CanError::RxEmpty)));
    }
}
