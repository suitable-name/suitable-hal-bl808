//! DSI (Display Serial Interface) driver for the BL808 D0 core.
//!
//! The DSI block transmits video frames to MIPI-DSI panels over D-PHY lanes.
//! Hardware base: 0x3001_a100 (D0 domain).

#[cfg(test)]
extern crate std;

#[cfg(not(test))]
use bl808_pac::Dsi as PacDsi;

#[cfg(all(test, feature = "d0"))]
std::thread_local! {
    pub static MOCK_DSI_PTR: core::cell::Cell<*const bl808_pac::dsi::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

#[inline]
fn regs() -> &'static bl808_pac::dsi::RegisterBlock {
    #[cfg(not(test))]
    return unsafe { &*PacDsi::PTR };
    #[cfg(test)]
    return MOCK_DSI_PTR.with(|p| unsafe { &*p.get() });
}

/// DSI data lane count.
///
/// Hardware encoding: `cr_lane_num` field in `DSI_CONFIG`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum DsiLanes {
    /// Single data lane.
    One = 0,
    /// Two data lanes.
    Two = 1,
    /// Four data lanes.
    Four = 2,
}

/// MIPI DSI data type codes (DSI spec Table 7, long-packet pixel formats).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum DsiDataType {
    Rgb565 = 0x0e,
    Rgb666Packed = 0x1e,
    Rgb666Loosely = 0x2e,
    Rgb888 = 0x3e,
}

/// DSI high-speed transmitter driver.
pub struct DsiDriver;

impl DsiDriver {
    pub fn new() -> Self {
        Self
    }

    /// Enable the HSTX (high-speed transmitter).
    pub fn enable(&mut self) {
        regs().dsi_config().modify(|_, w| w.cr_hstx_en().set_bit());
    }

    /// Disable the HSTX.
    pub fn disable(&mut self) {
        regs()
            .dsi_config()
            .modify(|_, w| w.cr_hstx_en().clear_bit());
    }

    /// Returns `true` if the HSTX is enabled.
    pub fn is_enabled(&self) -> bool {
        regs().dsi_config().read().cr_hstx_en().bit_is_set()
    }

    /// Configure number of MIPI data lanes.
    pub fn set_lane_count(&mut self, lanes: DsiLanes) {
        regs()
            .dsi_config()
            .modify(|_, w| unsafe { w.cr_lane_num().bits(lanes as u8) });
    }

    /// Read back the raw lane-count field (0=1-lane, 1=2-lane, 2=4-lane).
    pub fn lane_count_raw(&self) -> u8 {
        regs().dsi_config().read().cr_lane_num().bits()
    }

    /// Set DSI pixel data type.
    pub fn set_data_type(&mut self, dt: DsiDataType) {
        regs()
            .dsi_config()
            .modify(|_, w| unsafe { w.cr_dt().bits(dt as u8 & 0x3f) });
    }

    /// Set virtual channel number (0–3).
    pub fn set_virtual_channel(&mut self, vc: u8) {
        regs()
            .dsi_config()
            .modify(|_, w| unsafe { w.cr_vc().bits(vc & 0x3) });
    }

    /// Set vertical front porch width in lines.
    pub fn set_vfp(&mut self, vfp: u8) {
        regs()
            .dsi_config()
            .modify(|_, w| unsafe { w.cr_hstx_vfp().bits(vfp) });
    }

    /// Set vertical sync active width in lines.
    pub fn set_vsa(&mut self, vsa: u8) {
        regs()
            .dsi_config()
            .modify(|_, w| unsafe { w.cr_hstx_vsa().bits(vsa) });
    }

    /// Set HSTX packet count (11-bit, max 2047).
    pub fn set_packet_count(&mut self, count: u16) {
        regs()
            .dsi_hstx_config()
            .modify(|_, w| unsafe { w.cr_hstx_pc().bits(count & 0x7ff) });
    }

    /// Set HSTX output FIFO threshold (11-bit, max 2047).
    pub fn set_output_threshold(&mut self, threshold: u16) {
        regs()
            .dsi_hstx_config()
            .modify(|_, w| unsafe { w.cr_hstx_out_th().bits(threshold & 0x7ff) });
    }
}

impl Default for DsiDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use std::boxed::Box;

    /// Allocate a zeroed mock register block (0x100 bytes = 64 u32 words).
    fn make_driver() -> DsiDriver {
        let mock = Box::leak(Box::new([0u32; 64]))
            as *mut [u32; 64]
            as *const bl808_pac::dsi::RegisterBlock;
        MOCK_DSI_PTR.with(|p| p.set(mock));
        DsiDriver::new()
    }

    #[test]
    fn test_dsi_enable_disable() {
        let mut drv = make_driver();
        assert!(!drv.is_enabled(), "initially disabled");
        drv.enable();
        assert!(drv.is_enabled(), "enabled after enable()");
        drv.disable();
        assert!(!drv.is_enabled(), "disabled after disable()");
    }

    #[test]
    fn test_dsi_lane_count() {
        let mut drv = make_driver();
        drv.set_lane_count(DsiLanes::Two);
        assert_eq!(drv.lane_count_raw(), DsiLanes::Two as u8);
        drv.set_lane_count(DsiLanes::Four);
        assert_eq!(drv.lane_count_raw(), DsiLanes::Four as u8);
        drv.set_lane_count(DsiLanes::One);
        assert_eq!(drv.lane_count_raw(), DsiLanes::One as u8);
    }

    #[test]
    fn test_dsi_data_type() {
        let mut drv = make_driver();
        drv.set_data_type(DsiDataType::Rgb888);
        assert_eq!(
            regs().dsi_config().read().cr_dt().bits(),
            DsiDataType::Rgb888 as u8 & 0x3f
        );
        drv.set_data_type(DsiDataType::Rgb565);
        assert_eq!(
            regs().dsi_config().read().cr_dt().bits(),
            DsiDataType::Rgb565 as u8 & 0x3f
        );
    }

    #[test]
    fn test_dsi_virtual_channel_masking() {
        let mut drv = make_driver();
        drv.set_virtual_channel(3);
        assert_eq!(regs().dsi_config().read().cr_vc().bits(), 3);
        // Values above 3 must be masked to 2 bits.
        drv.set_virtual_channel(7);
        assert_eq!(regs().dsi_config().read().cr_vc().bits(), 3);
    }

    #[test]
    fn test_dsi_timing_fields() {
        let mut drv = make_driver();
        drv.set_vfp(10);
        drv.set_vsa(5);
        let r = regs().dsi_config().read();
        assert_eq!(r.cr_hstx_vfp().bits(), 10);
        assert_eq!(r.cr_hstx_vsa().bits(), 5);
    }

    #[test]
    fn test_dsi_packet_config() {
        let mut drv = make_driver();
        drv.set_packet_count(480);
        drv.set_output_threshold(0x348);
        let r = regs().dsi_hstx_config().read();
        assert_eq!(r.cr_hstx_pc().bits(), 480);
        assert_eq!(r.cr_hstx_out_th().bits(), 0x348);
    }
}
