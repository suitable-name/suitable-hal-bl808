//! Wireless Fidelity (Wifi) Driver
//!
//! This driver implements basic control for the Wifi peripheral on the BL808 SoC,
//! including coexistence arbitration.

use bl808_pac::Wifi;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_WIFI_PTR: core::cell::Cell<*const bl808_pac::wifi::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait WifiInstance: core::ops::Deref<Target = bl808_pac::wifi::RegisterBlock> {
    fn ptr() -> *const bl808_pac::wifi::RegisterBlock;
}

impl WifiInstance for Wifi {
    fn ptr() -> *const bl808_pac::wifi::RegisterBlock {
        #[cfg(not(test))]
        return Wifi::PTR;
        #[cfg(test)]
        return MOCK_WIFI_PTR.with(|p| p.get());
    }
}

pub struct WifiDriver<WIFI> {
    _wifi: WIFI,
}

impl<WIFI: WifiInstance> WifiDriver<WIFI> {
    pub fn new(wifi: WIFI) -> Self {
        Self { _wifi: wifi }
    }

    fn regs(&self) -> &bl808_pac::wifi::RegisterBlock {
        unsafe { &*WIFI::ptr() }
    }

    /// Enable coexistence arbitration
    pub fn enable_coex(&mut self) {
        self.regs().coex_control_0().modify(|_, w| unsafe { w.bits(1) });
    }

    pub fn get_coex_state(&self) -> u32 {
        self.regs().coex_state().read().bits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::wifi::RegisterBlock;

    #[test]
    fn test_wifi_coex_enable() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_WIFI_PTR.with(|p| p.set(mock_ptr));

        let wifi = unsafe { Wifi::steal() };
        let mut driver = WifiDriver::new(wifi);

        driver.enable_coex();

        // Register 0x400 is coex_control_0
        assert_eq!(mock_data[0x400 / 4], 1);
    }
}
