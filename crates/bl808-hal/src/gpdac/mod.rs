//! General Purpose DAC (GPDAC) driver
//!
//! The GPDAC registers live inside the GPIP PAC register block at offsets
//! 0x40–0x4C. This driver accesses those registers via `bl808_pac::Gpip::PTR`
//! directly, which is disjoint from the GPADC registers (0x00–0x20) owned
//! by [`crate::gpip::Gpip`].

#[cfg(all(feature = "m0", not(test)))]
use bl808_pac::Gpip as PacGpip;

#[cfg(test)]
extern crate std;

// Mock GPIP register block pointer for unit tests (requires m0 PAC types).
#[cfg(all(test, feature = "m0"))]
std::thread_local! {
    pub static MOCK_GPDAC_GPIP_PTR: core::cell::Cell<*const bl808_pac::gpip::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// DAC output rate mode.
#[cfg(feature = "m0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GpdacMode {
    /// 32 kHz output rate.
    Rate32k = 0,
    /// 16 kHz output rate.
    Rate16k = 1,
    /// 8 kHz output rate.
    Rate8k = 2,
    /// 512 kHz output rate (DMA mode only).
    Rate512k = 3,
}

/// Channel A/B data source selector.
#[cfg(feature = "m0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GpdacSrc {
    /// Output value comes from a register write (direct mode).
    Register = 0,
    /// Output value driven by DMA.
    Dma = 1,
}

/// Output channel selector.
#[cfg(feature = "m0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    A,
    B,
}

/// GPDAC driver.
///
/// Accesses GPDAC registers (offsets 0x40–0x4C) inside the shared GPIP
/// register block. Construct with [`Gpdac::new`]; no PAC ownership token is
/// taken because the [`crate::gpip::Gpip`] driver already owns the singleton.
///
/// # Safety
/// This driver accesses only the GPDAC half of the register block
/// (0x40–0x4C). The GPADC half (0x00–0x20) is never touched here, so
/// concurrent use with `Gpip` is safe at the register level.
#[cfg(feature = "m0")]
pub struct Gpdac;

#[cfg(feature = "m0")]
impl Gpdac {
    /// Construct the GPDAC driver.
    ///
    /// This does **not** consume a PAC token. Call this only after the
    /// system has initialised the GPIP peripheral via `Gpip::new()`.
    pub fn new() -> Self {
        Self
    }

    #[inline(always)]
    fn regs(&self) -> &bl808_pac::gpip::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacGpip::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_GPDAC_GPIP_PTR.with(|p| p.get()) };
    }

    // ── Enable/disable ───────────────────────────────────────────────────────

    /// Enable the GPDAC output.
    pub fn enable(&mut self) {
        self.regs()
            .gpdac_config()
            .modify(|_, w| w.gpdac_en().set_bit());
    }

    /// Disable the GPDAC output.
    pub fn disable(&mut self) {
        self.regs()
            .gpdac_config()
            .modify(|_, w| w.gpdac_en().clear_bit());
    }

    // ── Configuration ────────────────────────────────────────────────────────

    /// Set the DAC output rate mode.
    pub fn set_mode(&mut self, mode: GpdacMode) {
        self.regs()
            .gpdac_config()
            .modify(|_, w| unsafe { w.gpdac_mode().bits(mode as u8) });
    }

    /// Select the data source for channel A.
    pub fn set_channel_a_source(&mut self, src: GpdacSrc) {
        self.regs()
            .gpdac_config()
            .modify(|_, w| unsafe { w.gpdac_ch_a_sel().bits(src as u8) });
    }

    /// Select the data source for channel B.
    pub fn set_channel_b_source(&mut self, src: GpdacSrc) {
        self.regs()
            .gpdac_config()
            .modify(|_, w| unsafe { w.gpdac_ch_b_sel().bits(src as u8) });
    }

    // ── DMA ─────────────────────────────────────────────────────────────────

    /// Enable GPDAC DMA TX.
    pub fn enable_dma(&mut self) {
        self.regs()
            .gpdac_dma_config()
            .modify(|_, w| w.gpdac_dma_tx_en().set_bit());
    }

    /// Disable GPDAC DMA TX.
    pub fn disable_dma(&mut self) {
        self.regs()
            .gpdac_dma_config()
            .modify(|_, w| w.gpdac_dma_tx_en().clear_bit());
    }

    /// Configure the DMA data format (0–11).
    pub fn set_dma_format(&mut self, fmt: u8) {
        self.regs()
            .gpdac_dma_config()
            .modify(|_, w| unsafe { w.gpdac_dma_format().bits(fmt & 0xF) });
    }

    /// Enable or disable MSB inversion in DMA data.
    pub fn set_dma_inv_msb(&mut self, inv: bool) {
        self.regs()
            .gpdac_dma_config()
            .modify(|_, w| w.gpdac_dma_inv_msb().bit(inv));
    }

    // ── Sample write ─────────────────────────────────────────────────────────

    /// Push a 32-bit DMA sample word into the TX FIFO.
    ///
    /// The exact encoding depends on the DMA format register. In the default
    /// dual-channel interleaved format: bits `[31:22]` = channel B,
    /// bits `[21:12]` = channel A (both 10-bit unsigned).
    pub fn write_dma_sample(&mut self, data: u32) {
        self.regs()
            .gpdac_dma_wdata()
            .write(|w| unsafe { w.gpdac_dma_wdata().bits(data) });
    }

    // ── TX FIFO status ───────────────────────────────────────────────────────

    /// Returns `true` if the TX FIFO is empty.
    pub fn is_tx_fifo_empty(&self) -> bool {
        self.regs()
            .gpdac_tx_fifo_status()
            .read()
            .tx_fifo_empty()
            .bit_is_set()
    }

    /// Returns `true` if the TX FIFO is full.
    pub fn is_tx_fifo_full(&self) -> bool {
        self.regs()
            .gpdac_tx_fifo_status()
            .read()
            .tx_fifo_full()
            .bit_is_set()
    }
}

#[cfg(feature = "m0")]
impl Default for Gpdac {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::gpip::RegisterBlock;

    /// RegisterBlock is 0x50 bytes. Use 24 u32s to cover all registers.
    fn make_mock() -> std::vec::Vec<u32> {
        std::vec![0u32; 24]
    }

    fn setup_mock(buf: &[u32]) -> Gpdac {
        let ptr = buf.as_ptr() as *const RegisterBlock;
        MOCK_GPDAC_GPIP_PTR.with(|p| p.set(ptr));
        Gpdac::new()
    }

    // gpdac_config at offset 0x40 = index 16
    const IDX_GPDAC_CONFIG: usize = 0x40 / 4;
    // gpdac_dma_config at offset 0x44 = index 17
    const IDX_GPDAC_DMA_CONFIG: usize = 0x44 / 4;
    // gpdac_dma_wdata at offset 0x48 = index 18
    const IDX_GPDAC_DMA_WDATA: usize = 0x48 / 4;
    // gpdac_tx_fifo_status at offset 0x4C = index 19
    const IDX_GPDAC_TX_FIFO_STATUS: usize = 0x4C / 4;

    #[test]
    fn test_enable_sets_bit0_of_gpdac_config() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.enable();
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_CONFIG)) };
        assert_eq!(val & 1, 1, "gpdac_en (bit 0) should be set");
    }

    #[test]
    fn test_disable_clears_bit0_of_gpdac_config() {
        let mut buf = make_mock();
        buf[IDX_GPDAC_CONFIG] = 1;
        let mut drv = setup_mock(&buf);
        drv.disable();
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_CONFIG)) };
        assert_eq!(val & 1, 0, "gpdac_en (bit 0) should be cleared");
    }

    #[test]
    fn test_set_mode_rate16k() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_mode(GpdacMode::Rate16k);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_CONFIG)) };
        // gpdac_mode at bits[10:8]
        assert_eq!((val >> 8) & 0x7, GpdacMode::Rate16k as u32);
    }

    #[test]
    fn test_set_mode_rate512k() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_mode(GpdacMode::Rate512k);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_CONFIG)) };
        assert_eq!((val >> 8) & 0x7, GpdacMode::Rate512k as u32);
    }

    #[test]
    fn test_set_channel_a_source_dma() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_channel_a_source(GpdacSrc::Dma);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_CONFIG)) };
        // gpdac_ch_a_sel at bits[19:16]
        assert_eq!((val >> 16) & 0xF, GpdacSrc::Dma as u32);
    }

    #[test]
    fn test_set_channel_b_source_dma() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_channel_b_source(GpdacSrc::Dma);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_CONFIG)) };
        // gpdac_ch_b_sel at bits[23:20]
        assert_eq!((val >> 20) & 0xF, GpdacSrc::Dma as u32);
    }

    #[test]
    fn test_enable_dma_sets_bit0_of_dma_config() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.enable_dma();
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_DMA_CONFIG)) };
        assert_eq!(val & 1, 1, "gpdac_dma_tx_en (bit 0) should be set");
    }

    #[test]
    fn test_write_dma_sample() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.write_dma_sample(0xDEAD_BEEF);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_DMA_WDATA)) };
        assert_eq!(val, 0xDEAD_BEEF);
    }

    #[test]
    fn test_is_tx_fifo_empty_reads_bit0_of_fifo_status() {
        let mut buf = make_mock();
        buf[IDX_GPDAC_TX_FIFO_STATUS] = 1; // tx_fifo_empty bit 0
        let drv = setup_mock(&buf);
        assert!(drv.is_tx_fifo_empty());
    }

    #[test]
    fn test_is_tx_fifo_full_reads_bit1_of_fifo_status() {
        let mut buf = make_mock();
        buf[IDX_GPDAC_TX_FIFO_STATUS] = 1 << 1; // tx_fifo_full bit 1
        let drv = setup_mock(&buf);
        assert!(drv.is_tx_fifo_full());
    }

    #[test]
    fn test_set_dma_format() {
        let buf = make_mock();
        let mut drv = setup_mock(&buf);
        drv.set_dma_format(7);
        let val = unsafe { core::ptr::read_volatile(buf.as_ptr().add(IDX_GPDAC_DMA_CONFIG)) };
        // gpdac_dma_format at bits[7:4]
        assert_eq!((val >> 4) & 0xF, 7);
    }
}
