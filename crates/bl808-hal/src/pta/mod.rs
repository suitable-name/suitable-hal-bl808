//! PTA (Packet Traffic Arbitration) driver.
//!
//! Manages coexistence between Bluetooth and WiFi, granting exclusive medium
//! access to whichever radio has the higher-priority active transaction.

use bl808_pac::Pta;
use core::marker::PhantomData;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_PTA_PTR: core::cell::Cell<*const bl808_pac::pta::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait PtaInstance: core::ops::Deref<Target = bl808_pac::pta::RegisterBlock> {
    fn ptr() -> *const bl808_pac::pta::RegisterBlock;
}

impl PtaInstance for Pta {
    fn ptr() -> *const bl808_pac::pta::RegisterBlock {
        #[cfg(not(test))]
        return Pta::PTR;
        #[cfg(test)]
        return MOCK_PTA_PTR.with(|p| p.get());
    }
}

pub struct PtaDriver<PTA> {
    _pta: PTA,
}

impl<PTA: PtaInstance> PtaDriver<PTA> {
    pub fn new(pta: PTA) -> Self {
        Self { _pta: pta }
    }

    #[inline]
    fn regs(&self) -> &bl808_pac::pta::RegisterBlock {
        unsafe { &*PTA::ptr() }
    }

    /// Read the peripheral silicon revision register.
    pub fn get_revision(&self) -> u32 {
        self.regs().revision().read().bits()
    }

    // -----------------------------------------------------------------------
    // Arbitration config registers
    // -----------------------------------------------------------------------

    /// Write arbitration configuration register 0 (full 32-bit word).
    ///
    /// Controls overall arbitration policy (priority scheme, timers, etc.).
    pub fn set_config_0(&mut self, config: u32) {
        self.regs()
            .config_0()
            .write(|w| unsafe { w.bits(config) });
    }

    /// Read arbitration configuration register 0.
    pub fn get_config_0(&self) -> u32 {
        self.regs().config_0().read().bits()
    }

    /// Write arbitration configuration register 2 (PTA_REG_CONFIG2 at offset 0x428).
    ///
    /// Controls traffic-class force-grant bits (overrides normal arbitration).
    pub fn set_config_2(&mut self, config: u32) {
        self.regs()
            .config_2()
            .write(|w| unsafe { w.bits(config) });
    }

    /// Read arbitration configuration register 2 (PTA_REG_CONFIG2 at offset 0x428).
    pub fn get_config_2(&self) -> u32 {
        self.regs().config_2().read().bits()
    }

    /// Deprecated alias for [`set_config_2`].
    #[inline(always)]
    pub fn set_config_1(&mut self, config: u32) {
        self.set_config_2(config);
    }

    /// Deprecated alias for [`get_config_2`].
    #[inline(always)]
    pub fn get_config_1(&self) -> u32 {
        self.get_config_2()
    }

    // -----------------------------------------------------------------------
    // Bluetooth registers
    // -----------------------------------------------------------------------

    /// Configure Bluetooth transmit priority / timing parameters.
    pub fn config_bt_tx(&mut self, priority: u32) {
        self.regs()
            .bluetooth_transmit()
            .write(|w| unsafe { w.bits(priority) });
    }

    /// Read back the Bluetooth transmit configuration.
    pub fn get_bt_tx(&self) -> u32 {
        self.regs().bluetooth_transmit().read().bits()
    }

    /// Set Bluetooth transmit abort mask.
    ///
    /// Bits set here allow an in-progress BT TX to be aborted when WiFi
    /// traffic matches the mask condition.
    pub fn set_bt_tx_abort(&mut self, mask: u32) {
        self.regs()
            .bt_tx_abort()
            .write(|w| unsafe { w.bits(mask) });
    }

    /// Configure Bluetooth receive priority / timing parameters.
    pub fn config_bt_rx(&mut self, priority: u32) {
        self.regs()
            .bluetooth_receive()
            .write(|w| unsafe { w.bits(priority) });
    }

    /// Read back the Bluetooth receive configuration.
    pub fn get_bt_rx(&self) -> u32 {
        self.regs().bluetooth_receive().read().bits()
    }

    /// Set Bluetooth receive abort mask.
    pub fn set_bt_rx_abort(&mut self, mask: u32) {
        self.regs()
            .bt_rx_abort()
            .write(|w| unsafe { w.bits(mask) });
    }

    // -----------------------------------------------------------------------
    // WiFi / WLAN registers
    // -----------------------------------------------------------------------

    /// Configure WiFi transmit priority / timing parameters.
    pub fn config_wifi_tx(&mut self, priority: u32) {
        self.regs()
            .wifi_transmit()
            .write(|w| unsafe { w.bits(priority) });
    }

    /// Read back the WiFi transmit configuration.
    pub fn get_wifi_tx(&self) -> u32 {
        self.regs().wifi_transmit().read().bits()
    }

    /// Set WLAN transmit abort mask.
    ///
    /// Bits set here allow an in-progress WLAN TX to be aborted when
    /// Bluetooth traffic matches the mask condition.
    pub fn set_wlan_tx_abort(&mut self, mask: u32) {
        self.regs()
            .wlan_tx_abort()
            .write(|w| unsafe { w.bits(mask) });
    }

    /// Configure WiFi receive priority / timing parameters.
    pub fn config_wifi_rx(&mut self, priority: u32) {
        self.regs()
            .wifi_receive()
            .write(|w| unsafe { w.bits(priority) });
    }

    /// Read back the WiFi receive configuration.
    pub fn get_wifi_rx(&self) -> u32 {
        self.regs().wifi_receive().read().bits()
    }

    /// Set WLAN receive abort mask.
    pub fn set_wlan_rx_abort(&mut self, mask: u32) {
        self.regs()
            .wlan_rx_abort()
            .write(|w| unsafe { w.bits(mask) });
    }
}

// Silence the unused PhantomData warning that would appear if PTA was not
// referenced directly (it is, via _pta: PTA).
#[allow(dead_code)]
struct _Phantom<T>(PhantomData<T>);

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::pta::RegisterBlock;
    use std::boxed::Box;

    /// Allocate a zeroed mock block large enough for the PTA register map.
    /// PTA RegisterBlock ends at 0x428 + 4 = 0x42c bytes ≈ 267 u32 words;
    /// we use 512 for alignment headroom.
    fn make_driver() -> PtaDriver<Pta> {
        let mock = Box::leak(Box::new([0u32; 512]))
            as *mut [u32; 512]
            as *const RegisterBlock;
        MOCK_PTA_PTR.with(|p| p.set(mock));
        let pta = unsafe { Pta::steal() };
        PtaDriver::new(pta)
    }

    #[test]
    fn test_pta_config_0_roundtrip() {
        let mut drv = make_driver();
        drv.set_config_0(0x12345678);
        assert_eq!(drv.get_config_0(), 0x12345678);
    }

    #[test]
    fn test_pta_config_2_roundtrip() {
        let mut drv = make_driver();
        drv.set_config_2(0xcafebabe);
        assert_eq!(drv.get_config_2(), 0xcafebabe);
    }

    #[test]
    fn test_pta_config_1_roundtrip() {
        let mut drv = make_driver();
        drv.set_config_1(0xdeadbeef);
        assert_eq!(drv.get_config_1(), 0xdeadbeef);
    }

    #[test]
    fn test_pta_bt_tx_config() {
        let mut drv = make_driver();
        drv.config_bt_tx(0xaabbccdd);
        assert_eq!(drv.get_bt_tx(), 0xaabbccdd);
    }

    #[test]
    fn test_pta_bt_rx_config() {
        let mut drv = make_driver();
        drv.config_bt_rx(0x11223344);
        assert_eq!(drv.get_bt_rx(), 0x11223344);
    }

    #[test]
    fn test_pta_wifi_tx_config() {
        let mut drv = make_driver();
        drv.config_wifi_tx(0x55667788);
        assert_eq!(drv.get_wifi_tx(), 0x55667788);
    }

    #[test]
    fn test_pta_wifi_rx_config() {
        let mut drv = make_driver();
        drv.config_wifi_rx(0x99aabbcc);
        assert_eq!(drv.get_wifi_rx(), 0x99aabbcc);
    }

    #[test]
    fn test_pta_abort_masks() {
        let mut drv = make_driver();
        drv.set_bt_tx_abort(0x0f);
        drv.set_bt_rx_abort(0xf0);
        drv.set_wlan_tx_abort(0x55);
        drv.set_wlan_rx_abort(0xaa);
        // Verify none of these clobber each other by re-reading bt_tx config.
        drv.config_bt_tx(0x1234);
        assert_eq!(drv.get_bt_tx(), 0x1234);
    }
}
