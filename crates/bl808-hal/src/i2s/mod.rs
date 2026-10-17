//! Inter-IC Sound (I2S) Driver
//!
//! Supports standard TX/RX operation and DMA-backed audio transfers.
//!
//! Register map (relative to I2S base):
//!   0x00  i2s_config       — master/slave, format, word-length, en bits
//!   0x04  i2s_int_sts      — interrupt status / enable
//!   0x10  i2s_bclk_config  — BCLK divider
//!   0x80  i2s_fifo_config_0 — DMA en, FIFO clear, LR options
//!   0x84  i2s_fifo_config_1 — FIFO thresholds / count
//!   0x88  i2s_fifo_wdata   — TX FIFO write port (DMA destination)
//!   0x8c  i2s_fifo_rdata   — RX FIFO read port  (DMA source)
//!   0xfc  i2s_io_config    — pin mux / data format
//!
//! # DMA usage
//!
//! The BL808 I2S uses DMA0 or DMA1 (peripheral request IDs from SDK `hal_dma.h`):
//! - `DMA_REQUEST_I2S_TX = 17`  (memory → peripheral)
//! - `DMA_REQUEST_I2S_RX = 16`  (peripheral → memory)
//!
//! The DMA FIFO threshold (`tx_fifo_th` / `rx_fifo_th` in `i2s_fifo_config_1`)
//! controls when the hardware asserts its DMA request line.  The SDK sets this
//! via `fifoCfg.txFifoLevel` / `fifoCfg.rxFifoLevel` (see `I2S_FifoConfig` in
//! `bl808_i2s.c`).  [`I2sTx::start_tx_dma`] and [`I2sRx::start_rx_dma`] accept
//! a `fifo_threshold` argument that is written to the same register so the
//! threshold is always programmed before the DMA request is enabled.
//!
//! ## Circular streaming example
//!
//! ```ignore
//! use bl808_hal::dma::{
//!     build_circular_lli, BurstSize, DmaDirection, FlowControl, Lli, TransferWidth,
//! };
//!
//! const HALF: usize = 512;
//! static mut BUF_A: [u32; HALF] = [0u32; HALF];
//! static mut BUF_B: [u32; HALF] = [0u32; HALF];
//! static mut RING: [Lli; 2] = [Lli { src_addr: 0, dst_addr: 0, next_lli: 0, control: 0 }; 2];
//!
//! // 1. Enable the I2S TX path and set DMA mode + FIFO threshold.
//! //    fifo_threshold = 3 means: assert DMA request when ≥ 4 slots are free.
//! tx.start_tx_dma(3);
//!
//! // 2. Build the circular LLI ring over the two audio buffers.
//! let segs = unsafe {
//!     [(BUF_A.as_ptr() as u32, HALF as u16),
//!      (BUF_B.as_ptr() as u32, HALF as u16)]
//! };
//! unsafe {
//!     build_circular_lli(
//!         &mut RING,
//!         &segs,
//!         tx.tx_fifo_addr(),
//!         DmaDirection::MemToPeri,
//!         TransferWidth::Bits32,
//!         BurstSize::Burst4,
//!     );
//!
//!     // 3. Start the DMA channel with the correct flow-control and
//!     //    I2S-TX peripheral ID (17 = DMA_REQUEST_I2S_TX).
//!     dma_channel.start_lli_peripheral(
//!         &RING[0],
//!         FlowControl::Mem2Peri,
//!         0,   // src_peripheral (unused for Mem→Peri)
//!         17,  // dst_peripheral = I2S TX
//!     ).unwrap();
//! }
//!
//! // For RX — mirror pattern:
//! //   rx.start_rx_dma(3);
//! //   build_circular_lli(&mut RING, &segs, rx.rx_fifo_addr(),
//! //                      DmaDirection::PeriToMem, ...);
//! //   dma_channel.start_lli_peripheral(&RING[0],
//! //       FlowControl::Peri2Mem, 16 /*I2S_RX*/, 0).unwrap();
//! ```

use bl808_pac::I2s;
use core::marker::PhantomData;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_I2S_PTR: core::cell::Cell<*const bl808_pac::i2s::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait I2sInstance: core::ops::Deref<Target = bl808_pac::i2s::RegisterBlock> {
    fn ptr() -> *const bl808_pac::i2s::RegisterBlock;
}

impl I2sInstance for I2s {
    fn ptr() -> *const bl808_pac::i2s::RegisterBlock {
        #[cfg(not(test))]
        return I2s::PTR;
        #[cfg(test)]
        return MOCK_I2S_PTR.with(|p| p.get());
    }
}

/// Physical offset of the TX FIFO write port within the I2S register block.
const TX_FIFO_OFFSET: usize = 0x88;
/// Physical offset of the RX FIFO read port within the I2S register block.
const RX_FIFO_OFFSET: usize = 0x8C;

pub struct I2sDriver<I2S> {
    _i2s: I2S,
}

impl<I2S: I2sInstance> I2sDriver<I2S> {
    pub fn new(i2s: I2S) -> Self {
        Self { _i2s: i2s }
    }

    pub fn split(self) -> (I2sTx<I2S>, I2sRx<I2S>) {
        (I2sTx { _marker: PhantomData }, I2sRx { _marker: PhantomData })
    }
}

// ---------------------------------------------------------------------------
// TX half
// ---------------------------------------------------------------------------

pub struct I2sTx<I2S> {
    _marker: PhantomData<I2S>,
}

impl<I2S: I2sInstance> I2sTx<I2S> {
    fn regs(&self) -> &bl808_pac::i2s::RegisterBlock {
        unsafe { &*I2S::ptr() }
    }

    /// Enable the I2S TX path (sets cr_i2s_tx_en in i2s_config).
    pub fn enable(&mut self) {
        self.regs().i2s_config().modify(|_, w| w.cr_i2s_txd_en().set_bit());
    }

    /// Disable the I2S TX path.
    pub fn disable(&mut self) {
        self.regs().i2s_config().modify(|_, w| w.cr_i2s_txd_en().clear_bit());
    }

    /// Write one sample word directly into the TX FIFO (CPU-driven).
    pub fn write_fifo(&mut self, data: u32) {
        self.regs().i2s_fifo_wdata().write(|w| unsafe { w.bits(data) });
    }

    /// Enable the TX DMA request line, set the FIFO DMA threshold, and clear
    /// the TX FIFO.
    ///
    /// `fifo_threshold` is the value written to the `tx_fifo_th` field of
    /// `i2s_fifo_config_1` (4 bits, values 0–15).  The hardware raises its DMA
    /// request when the number of *free* slots in the TX FIFO exceeds this
    /// threshold.  The SDK uses `i2s_device->fifo_threshold` here; a value of
    /// **3** (request when ≥ 4 slots free) works well for 32-bit stereo at
    /// Burst-4.
    ///
    /// After calling this, build a circular LLI ring over your audio buffers
    /// using [`build_circular_lli`] and start the DMA channel with
    /// [`Channel::start_lli_peripheral`].  The peripheral FIFO destination
    /// address is `tx_fifo_addr()`, the flow-control is `Mem2Peri`, and the
    /// destination peripheral ID is **17** (`DMA_REQUEST_I2S_TX` from the SDK).
    ///
    /// [`build_circular_lli`]: crate::dma::build_circular_lli
    /// [`Channel::start_lli_peripheral`]: crate::dma::Channel::start_lli_peripheral
    pub fn start_tx_dma(&mut self, fifo_threshold: u8) {
        // Set FIFO DMA threshold in fifo_config_1 (SDK: I2S_FifoConfig →
        // BL_SET_REG_BITS_VAL(tmpVal, I2S_TX_FIFO_TH, fifoCfg->txFifoLevel)).
        self.regs()
            .i2s_fifo_config_1()
            .modify(|_, w| unsafe { w.tx_fifo_th().bits(fifo_threshold & 0xF) });

        // Clear TX FIFO (self-clearing pulse) and enable DMA request line.
        // SDK sequence from I2S_FifoConfig:
        //   1. Write FIFO_CLR=1 + DMA_TX_EN=1 together.
        //   2. Clear FIFO_CLR back to 0.
        // The PAC models the clear bit as write-1-to-clear so a single modify
        // is equivalent: set DMA_TX_EN, pulse TX_FIFO_CLR.
        self.regs().i2s_fifo_config_0().modify(|_, w| {
            w.tx_fifo_clr()
                .clear_bit_by_one()
                .i2s_dma_tx_en()
                .set_bit()
        });
    }

    /// Disable the TX DMA request line.
    pub fn stop_tx_dma(&mut self) {
        self.regs()
            .i2s_fifo_config_0()
            .modify(|_, w| w.i2s_dma_tx_en().clear_bit());
    }

    /// Physical address of the TX FIFO write port.
    ///
    /// Pass this to your DMA engine as the peripheral destination address
    /// (use with `DmaDirection::MemToPeri` in [`build_circular_lli`]).
    ///
    /// [`build_circular_lli`]: crate::dma::build_circular_lli
    pub fn tx_fifo_addr(&self) -> u32 {
        (I2S::ptr() as usize + TX_FIFO_OFFSET) as u32
    }

    /// `true` when the TX FIFO has overflowed.
    pub fn is_overflow(&self) -> bool {
        self.regs().i2s_fifo_config_0().read().tx_fifo_overflow().bit_is_set()
    }

    /// `true` when the TX FIFO has underflowed.
    pub fn is_underflow(&self) -> bool {
        self.regs().i2s_fifo_config_0().read().tx_fifo_underflow().bit_is_set()
    }
}

// ---------------------------------------------------------------------------
// RX half
// ---------------------------------------------------------------------------

pub struct I2sRx<I2S> {
    _marker: PhantomData<I2S>,
}

impl<I2S: I2sInstance> I2sRx<I2S> {
    fn regs(&self) -> &bl808_pac::i2s::RegisterBlock {
        unsafe { &*I2S::ptr() }
    }

    /// Enable the I2S RX path (sets cr_i2s_rx_en in i2s_config).
    pub fn enable(&mut self) {
        self.regs().i2s_config().modify(|_, w| w.cr_i2s_rxd_en().set_bit());
    }

    /// Disable the I2S RX path.
    pub fn disable(&mut self) {
        self.regs().i2s_config().modify(|_, w| w.cr_i2s_rxd_en().clear_bit());
    }

    /// Read one sample word directly from the RX FIFO (CPU-driven).
    pub fn read_fifo(&self) -> u32 {
        self.regs().i2s_fifo_rdata().read().bits()
    }

    /// Enable the RX DMA request line, set the FIFO DMA threshold, and clear
    /// the RX FIFO.
    ///
    /// `fifo_threshold` is the value written to the `rx_fifo_th` field of
    /// `i2s_fifo_config_1` (4 bits, values 0–15).  The hardware raises its DMA
    /// request when the number of *filled* slots in the RX FIFO exceeds this
    /// threshold.  The SDK uses `i2s_device->fifo_threshold` here; a value of
    /// **3** works well for 32-bit stereo at Burst-4.
    ///
    /// After calling this, build a circular LLI ring over your audio buffers
    /// using [`build_circular_lli`] and start the DMA channel with
    /// [`Channel::start_lli_peripheral`].  The peripheral FIFO source address
    /// is `rx_fifo_addr()`, the flow-control is `Peri2Mem`, and the source
    /// peripheral ID is **16** (`DMA_REQUEST_I2S_RX` from the SDK).
    ///
    /// [`build_circular_lli`]: crate::dma::build_circular_lli
    /// [`Channel::start_lli_peripheral`]: crate::dma::Channel::start_lli_peripheral
    pub fn start_rx_dma(&mut self, fifo_threshold: u8) {
        // Set FIFO DMA threshold in fifo_config_1 (SDK: I2S_FifoConfig →
        // BL_SET_REG_BITS_VAL(tmpVal, I2S_RX_FIFO_TH, fifoCfg->rxFifoLevel)).
        self.regs()
            .i2s_fifo_config_1()
            .modify(|_, w| unsafe { w.rx_fifo_th().bits(fifo_threshold & 0xF) });

        // Clear RX FIFO and enable DMA request line (same SDK sequence as TX).
        self.regs().i2s_fifo_config_0().modify(|_, w| {
            w.rx_fifo_clr()
                .clear_bit_by_one()
                .i2s_dma_rx_en()
                .set_bit()
        });
    }

    /// Disable the RX DMA request line.
    pub fn stop_rx_dma(&mut self) {
        self.regs()
            .i2s_fifo_config_0()
            .modify(|_, w| w.i2s_dma_rx_en().clear_bit());
    }

    /// Physical address of the RX FIFO read port.
    ///
    /// Pass this to your DMA engine as the peripheral source address
    /// (use with `DmaDirection::PeriToMem` in [`build_circular_lli`]).
    ///
    /// [`build_circular_lli`]: crate::dma::build_circular_lli
    pub fn rx_fifo_addr(&self) -> u32 {
        (I2S::ptr() as usize + RX_FIFO_OFFSET) as u32
    }

    /// `true` when the RX FIFO has overflowed.
    pub fn is_overflow(&self) -> bool {
        self.regs().i2s_fifo_config_0().read().rx_fifo_overflow().bit_is_set()
    }

    /// `true` when the RX FIFO has underflowed.
    pub fn is_underflow(&self) -> bool {
        self.regs().i2s_fifo_config_0().read().rx_fifo_underflow().bit_is_set()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::i2s::RegisterBlock;

    fn setup() -> ([u32; 1024], *const RegisterBlock) {
        let mock = [0u32; 1024];
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_I2S_PTR.with(|p| p.set(ptr));
        (mock, ptr)
    }

    #[test]
    fn test_i2s_tx_enable_sets_bit() {
        let (mock, ptr) = setup();
        let i2s = unsafe { I2s::steal() };
        let driver = I2sDriver::new(i2s);
        let (mut tx, _) = driver.split();

        tx.enable();

        let regs = unsafe { &*ptr };
        assert!(
            regs.i2s_config().read().cr_i2s_txd_en().bit_is_set(),
            "cr_i2s_txd_en should be set"
        );

        let _ = mock;
    }

    #[test]
    fn test_i2s_start_tx_dma_enables_dma_bit() {
        let (mock, ptr) = setup();
        let i2s = unsafe { I2s::steal() };
        let driver = I2sDriver::new(i2s);
        let (mut tx, _) = driver.split();

        tx.start_tx_dma(3);

        // i2s_fifo_config_0 at 0x80 (word offset 0x80/4 = 32)
        let val = unsafe { core::ptr::read_volatile((ptr as *const u32).add(0x80 / 4)) };
        // i2s_dma_tx_en = bit 0
        assert_ne!(val & (1 << 0), 0, "DMA TX enable bit should be set");

        let _ = mock;
    }

    #[test]
    fn test_i2s_start_tx_dma_sets_fifo_threshold() {
        let (mock, ptr) = setup();
        let i2s = unsafe { I2s::steal() };
        let driver = I2sDriver::new(i2s);
        let (mut tx, _) = driver.split();

        // Use threshold value 5.
        tx.start_tx_dma(5);

        // i2s_fifo_config_1 at 0x84 (word offset 0x84/4 = 33)
        // tx_fifo_th is at bits [19:16] per PAC i2s_fifo_config_1.rs.
        let val = unsafe { core::ptr::read_volatile((ptr as *const u32).add(0x84 / 4)) };
        let tx_th = (val >> 16) & 0xF;
        assert_eq!(tx_th, 5, "TX FIFO threshold should be 5");

        let _ = mock;
    }

    #[test]
    fn test_i2s_start_rx_dma_enables_dma_bit() {
        let (mock, ptr) = setup();
        let i2s = unsafe { I2s::steal() };
        let driver = I2sDriver::new(i2s);
        let (_, mut rx) = driver.split();

        rx.start_rx_dma(3);

        // i2s_dma_rx_en = bit 1 of i2s_fifo_config_0
        let val = unsafe { core::ptr::read_volatile((ptr as *const u32).add(0x80 / 4)) };
        assert_ne!(val & (1 << 1), 0, "DMA RX enable bit should be set");

        let _ = mock;
    }

    #[test]
    fn test_i2s_start_rx_dma_sets_fifo_threshold() {
        let (mock, ptr) = setup();
        let i2s = unsafe { I2s::steal() };
        let driver = I2sDriver::new(i2s);
        let (_, mut rx) = driver.split();

        // Use threshold value 7.
        rx.start_rx_dma(7);

        // i2s_fifo_config_1 at 0x84; rx_fifo_th at bits [27:24].
        let val = unsafe { core::ptr::read_volatile((ptr as *const u32).add(0x84 / 4)) };
        let rx_th = (val >> 24) & 0xF;
        assert_eq!(rx_th, 7, "RX FIFO threshold should be 7");

        let _ = mock;
    }

    #[test]
    fn test_i2s_fifo_addr_offsets() {
        let (mock, ptr) = setup();
        MOCK_I2S_PTR.with(|p| p.set(ptr));
        let i2s = unsafe { I2s::steal() };
        let driver = I2sDriver::new(i2s);
        let (tx, rx) = driver.split();

        assert_eq!(tx.tx_fifo_addr(), ptr as u32 + 0x88);
        assert_eq!(rx.rx_fifo_addr(), ptr as u32 + 0x8C);

        let _ = mock;
    }
}
