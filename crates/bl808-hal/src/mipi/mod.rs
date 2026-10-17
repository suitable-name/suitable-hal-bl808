//! MIPI CSI-2 (Camera Serial Interface) driver for the BL808 D0 core.
//!
//! The CSI block receives video frames from image sensors over MIPI D-PHY
//! lanes and routes them to DVP output channels.
//! Hardware base: 0x3001_a000 (D0 domain).

#[cfg(test)]
extern crate std;

#[cfg(not(test))]
use bl808_pac::Csi as PacCsi;

#[cfg(all(test, feature = "d0"))]
std::thread_local! {
    pub static MOCK_CSI_PTR: core::cell::Cell<*const bl808_pac::csi::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

#[inline]
fn regs() -> &'static bl808_pac::csi::RegisterBlock {
    #[cfg(not(test))]
    return unsafe { &*PacCsi::PTR };
    #[cfg(test)]
    return MOCK_CSI_PTR.with(|p| unsafe { &*p.get() });
}

/// MIPI CSI-2 receiver driver.
pub struct MipiDriver;

impl MipiDriver {
    pub fn new() -> Self {
        Self
    }

    /// Enable the MIPI CSI module.
    pub fn enable(&mut self) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_csi_en().set_bit());
    }

    /// Disable the MIPI CSI module.
    pub fn disable(&mut self) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_csi_en().clear_bit());
    }

    /// Returns `true` if the CSI module is enabled.
    pub fn is_enabled(&self) -> bool {
        regs().mipi_config().read().cr_csi_en().bit_is_set()
    }

    /// Select lane count: `false` = 1 lane, `true` = 2 lanes.
    pub fn set_dual_lane(&mut self, dual: bool) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_lane_num().bit(dual));
    }

    /// Invert the physical lane order.
    pub fn set_lane_invert(&mut self, invert: bool) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_lane_inv().bit(invert));
    }

    /// Invert data bit order within each byte.
    pub fn set_data_bit_invert(&mut self, invert: bool) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_data_bit_inv().bit(invert));
    }

    /// Enable sync short-packet mode.
    pub fn set_sync_short_packet(&mut self, enable: bool) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_sync_sp_en().bit(enable));
    }

    /// Enable pixel-data unpacking (e.g. 10-bit packed RAW).
    pub fn set_unpack(&mut self, enable: bool) {
        regs()
            .mipi_config()
            .modify(|_, w| w.cr_unpack_en().bit(enable));
    }

    /// Route virtual channel `vc` (0–3) to DVP0 output.
    pub fn set_vc_dvp0(&mut self, vc: u8) {
        regs()
            .mipi_config()
            .modify(|_, w| unsafe { w.cr_vc_dvp0().bits(vc & 0x3) });
    }

    /// Route virtual channel `vc` (0–3) to DVP1 output.
    pub fn set_vc_dvp1(&mut self, vc: u8) {
        regs()
            .mipi_config()
            .modify(|_, w| unsafe { w.cr_vc_dvp1().bits(vc & 0x3) });
    }

    /// Return `true` if the generic buffer contains unread data.
    pub fn gnr_buf_ready(&self) -> bool {
        regs().gnr_buf_status().read().bits() & 1 != 0
    }

    /// Read one word from the generic (command-response) buffer.
    pub fn gnr_buf_read(&self) -> u32 {
        regs().gnr_buf_rdata().read().bits()
    }
}

impl Default for MipiDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use std::boxed::Box;

    /// Allocate a zeroed mock register block (0x100 bytes = 64 u32 words).
    fn make_driver() -> MipiDriver {
        let mock = Box::leak(Box::new([0u32; 64]))
            as *mut [u32; 64]
            as *const bl808_pac::csi::RegisterBlock;
        MOCK_CSI_PTR.with(|p| p.set(mock));
        MipiDriver::new()
    }

    #[test]
    fn test_mipi_enable_disable() {
        let mut drv = make_driver();
        assert!(!drv.is_enabled(), "initially disabled");
        drv.enable();
        assert!(drv.is_enabled(), "enabled after enable()");
        drv.disable();
        assert!(!drv.is_enabled(), "disabled after disable()");
    }

    #[test]
    fn test_mipi_dual_lane() {
        let mut drv = make_driver();
        drv.set_dual_lane(true);
        assert!(regs().mipi_config().read().cr_lane_num().bit_is_set());
        drv.set_dual_lane(false);
        assert!(regs().mipi_config().read().cr_lane_num().bit_is_clear());
    }

    #[test]
    fn test_mipi_lane_and_bit_invert() {
        let mut drv = make_driver();
        drv.set_lane_invert(true);
        drv.set_data_bit_invert(true);
        let r = regs().mipi_config().read();
        assert!(r.cr_lane_inv().bit_is_set());
        assert!(r.cr_data_bit_inv().bit_is_set());
        drv.set_lane_invert(false);
        assert!(regs().mipi_config().read().cr_lane_inv().bit_is_clear());
    }

    #[test]
    fn test_mipi_vc_routing() {
        let mut drv = make_driver();
        drv.set_vc_dvp0(1);
        drv.set_vc_dvp1(2);
        let r = regs().mipi_config().read();
        assert_eq!(r.cr_vc_dvp0().bits(), 1);
        assert_eq!(r.cr_vc_dvp1().bits(), 2);
        // Masking: values > 3 are clipped to 2 bits.
        drv.set_vc_dvp0(7);
        assert_eq!(regs().mipi_config().read().cr_vc_dvp0().bits(), 3);
    }

    #[test]
    fn test_mipi_sync_and_unpack() {
        let mut drv = make_driver();
        drv.set_sync_short_packet(true);
        drv.set_unpack(true);
        let r = regs().mipi_config().read();
        assert!(r.cr_sync_sp_en().bit_is_set());
        assert!(r.cr_unpack_en().bit_is_set());
    }
}
