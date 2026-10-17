//! GPADC Peripheral Interface (GPIP) — GPADC driver
//!
//! Covers the GPADC side of the shared GPIP register block.
//! GPDAC registers (offset 0x40+) are handled by the `gpdac` module.

#[cfg(feature = "m0")]
use bl808_pac::Gpip as PacGpip;

#[cfg(test)]
extern crate std;

// Mock register block pointer for unit tests (requires m0 PAC types).
#[cfg(all(test, feature = "m0"))]
std::thread_local! {
    pub static MOCK_GPIP_PTR: core::cell::Cell<*const bl808_pac::gpip::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// FIFO interrupt threshold levels.
#[cfg(feature = "m0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FifoThreshold {
    /// Trigger after 1 sample.
    One = 0,
    /// Trigger after 4 samples.
    Four = 1,
    /// Trigger after 8 samples.
    Eight = 2,
    /// Trigger after 16 samples.
    Sixteen = 3,
}

/// GPADC (ADC FIFO) driver backed by the GPIP PAC register block.
///
/// Owns the `Gpip` PAC singleton. The GPDAC side is managed by [`crate::gpdac::Gpdac`],
/// which accesses the same register block at disjoint offsets (0x40–0x4C).
#[cfg(feature = "m0")]
pub struct Gpip {
    _inner: PacGpip,
}

#[cfg(feature = "m0")]
impl Gpip {
    /// Create from the PAC peripheral singleton.
    pub fn new(gpip: PacGpip) -> Self {
        Self { _inner: gpip }
    }

    #[inline(always)]
    fn regs(&self) -> &bl808_pac::gpip::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacGpip::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_GPIP_PTR.with(|p| p.get()) };
    }

    // ── FIFO control ────────────────────────────────────────────────────────

    /// Set the GPADC FIFO interrupt threshold level.
    pub fn set_fifo_threshold(&mut self, thl: FifoThreshold) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| unsafe { w.gpadc_fifo_thl().bits(thl as u8) });
    }

    /// Clear the GPADC FIFO (write-1-to-clear).
    pub fn clear_fifo(&mut self) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_fifo_clr().clear_bit_by_one());
    }

    /// Returns `true` if the FIFO contains at least one sample.
    pub fn is_fifo_not_empty(&self) -> bool {
        self.regs()
            .gpadc_config()
            .read()
            .gpadc_fifo_ne()
            .bit_is_set()
    }

    /// Returns `true` if the FIFO is full.
    pub fn is_fifo_full(&self) -> bool {
        self.regs()
            .gpadc_config()
            .read()
            .gpadc_fifo_full()
            .bit_is_set()
    }

    /// Number of samples currently in the FIFO (0–63).
    pub fn fifo_data_count(&self) -> u8 {
        self.regs()
            .gpadc_config()
            .read()
            .gpadc_fifo_data_count()
            .bits()
    }

    // ── DMA ─────────────────────────────────────────────────────────────────

    /// Enable GPADC DMA requests.
    pub fn enable_dma(&mut self) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_dma_en().set_bit());
    }

    /// Disable GPADC DMA requests.
    pub fn disable_dma(&mut self) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_dma_en().clear_bit());
    }

    // ── Sample read ─────────────────────────────────────────────────────────

    /// Pop one sample from the FIFO.
    ///
    /// Returns `None` if the FIFO is empty. The 26-bit value encodes
    /// `[25:12]` channel ID and `[11:0]` raw ADC result.
    pub fn read_sample(&self) -> Option<u32> {
        if !self.is_fifo_not_empty() {
            return None;
        }
        Some(
            self.regs()
                .gpadc_dma_rdata()
                .read()
                .gpadc_dma_rdata()
                .bits(),
        )
    }

    // ── Interrupt status ────────────────────────────────────────────────────

    /// Returns `true` if a conversion-ready interrupt is pending.
    pub fn is_conversion_done(&self) -> bool {
        self.regs()
            .gpadc_config()
            .read()
            .gpadc_rdy()
            .bit_is_set()
    }

    /// Returns `true` if a FIFO overrun interrupt is pending.
    pub fn is_overrun(&self) -> bool {
        self.regs()
            .gpadc_config()
            .read()
            .gpadc_fifo_overrun()
            .bit_is_set()
    }

    /// Returns `true` if a FIFO underrun interrupt is pending.
    pub fn is_underrun(&self) -> bool {
        self.regs()
            .gpadc_config()
            .read()
            .gpadc_fifo_underrun()
            .bit_is_set()
    }

    // ── Interrupt clear ─────────────────────────────────────────────────────

    /// Clear the conversion-ready interrupt flag.
    pub fn clear_ready_irq(&mut self) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_rdy_clr().set_bit());
    }

    /// Clear the FIFO overrun interrupt flag.
    pub fn clear_overrun_irq(&mut self) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_fifo_overrun_clr().set_bit());
    }

    /// Clear the FIFO underrun interrupt flag.
    pub fn clear_underrun_irq(&mut self) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_fifo_underrun_clr().set_bit());
    }

    // ── Interrupt mask ──────────────────────────────────────────────────────

    /// Mask or unmask the conversion-ready interrupt.
    pub fn set_ready_irq_mask(&mut self, masked: bool) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_rdy_mask().bit(masked));
    }

    /// Mask or unmask the FIFO overrun interrupt.
    pub fn set_overrun_irq_mask(&mut self, masked: bool) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_fifo_overrun_mask().bit(masked));
    }

    /// Mask or unmask the FIFO underrun interrupt.
    pub fn set_underrun_irq_mask(&mut self, masked: bool) {
        self.regs()
            .gpadc_config()
            .modify(|_, w| w.gpadc_fifo_underrun_mask().bit(masked));
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::{gpip::RegisterBlock, Gpip as PacGpip};

    /// RegisterBlock is 0x50 bytes = 20 u32s. Use 32 for headroom.
    fn make_mock() -> std::vec::Vec<u32> {
        std::vec![0u32; 32]
    }

    fn setup_mock(buf: &[u32]) -> Gpip {
        let ptr = buf.as_ptr() as *const RegisterBlock;
        MOCK_GPIP_PTR.with(|p| p.set(ptr));
        let gpip = unsafe { PacGpip::steal() };
        Gpip::new(gpip)
    }

    #[test]
    fn test_fifo_threshold_sixteen() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_fifo_threshold(FifoThreshold::Sixteen);
        // gpadc_config offset 0x00, fifo_thl at bits[23:22]
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr()) };
        assert_eq!((val >> 22) & 0x3, FifoThreshold::Sixteen as u32);
    }

    #[test]
    fn test_fifo_threshold_four() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_fifo_threshold(FifoThreshold::Four);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr()) };
        assert_eq!((val >> 22) & 0x3, FifoThreshold::Four as u32);
    }

    #[test]
    fn test_enable_dma_sets_bit0() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.enable_dma();
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr()) };
        assert_eq!(val & 1, 1, "gpadc_dma_en should be set");
    }

    #[test]
    fn test_disable_dma_clears_bit0() {
        let mut buf = make_mock();
        buf[0] = 1; // pre-set DMA enable
        let mut drv = setup_mock(&buf);
        drv.disable_dma();
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr()) };
        assert_eq!(val & 1, 0, "gpadc_dma_en should be cleared");
    }

    #[test]
    fn test_is_fifo_not_empty_false_when_zero() {
        let buf = make_mock();
        let drv = setup_mock(&buf);
        assert!(!drv.is_fifo_not_empty());
    }

    #[test]
    fn test_is_fifo_not_empty_true_when_bit2_set() {
        let mut buf = make_mock();
        buf[0] = 1 << 2; // gpadc_fifo_ne at bit 2
        let drv = setup_mock(&buf);
        assert!(drv.is_fifo_not_empty());
    }

    #[test]
    fn test_read_sample_returns_none_when_empty() {
        let buf = make_mock();
        let drv = setup_mock(&buf);
        assert!(drv.read_sample().is_none());
    }

    #[test]
    fn test_read_sample_returns_value_when_fifo_ne() {
        let mut buf = make_mock();
        buf[0] = 1 << 2; // gpadc_fifo_ne
        // gpadc_dma_rdata at offset 0x04 = index 1
        buf[1] = 0x03AB_1234 & 0x03FF_FFFF;
        let drv = setup_mock(&buf);
        let sample = drv.read_sample().expect("should have sample");
        assert_eq!(sample, buf[1] & 0x03FF_FFFF);
    }

    #[test]
    fn test_is_conversion_done_reads_bit4() {
        let mut buf = make_mock();
        buf[0] = 1 << 4; // gpadc_rdy at bit 4
        let drv = setup_mock(&buf);
        assert!(drv.is_conversion_done());
    }

    #[test]
    fn test_clear_ready_irq_writes_bit8() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.clear_ready_irq();
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr()) };
        assert_ne!(val & (1 << 8), 0, "gpadc_rdy_clr (bit 8) should be written");
    }

    #[test]
    fn test_fifo_data_count_reads_bits16_21() {
        let mut buf = make_mock();
        buf[0] = 5 << 16; // fifo_data_count = 5
        let drv = setup_mock(&buf);
        assert_eq!(drv.fifo_data_count(), 5);
    }
}
