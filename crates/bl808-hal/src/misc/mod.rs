#[cfg(feature = "m0")]
use bl808_pac::McuMisc as PacMisc;

#[cfg(test)]
extern crate std;

#[cfg(all(test, feature = "m0"))]
std::thread_local! {
    pub static MOCK_MISC_PTR: core::cell::Cell<*const bl808_pac::mcu_misc::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(feature = "m0")]
pub struct McuMisc {
    _inner: PacMisc,
}

#[cfg(feature = "m0")]
impl McuMisc {
    /// Construct the MCU MISC driver from the PAC peripheral token.
    pub fn new(misc: PacMisc) -> Self {
        Self { _inner: misc }
    }

    fn regs(&self) -> &bl808_pac::mcu_misc::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacMisc::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_MISC_PTR.with(|p| p.get()) };
    }

    /// Read the E907 fault-cause log register (`MCU1_LOG1`, offset 0x110).
    ///
    /// Contains the MCAUSE CSR value captured on the last exception/interrupt.
    /// Non-zero values indicate a pending fault; zero means no fault has been
    /// recorded since reset.
    pub fn get_error_status(&self) -> u32 {
        self.regs().mcu1_log1().read().bits()
    }

    /// Read the MCU bus configuration register 0 (`MCU_BUS_CFG0`, offset 0x00).
    ///
    /// Encodes the AHB bus timeout counter value for the E907 MCU.
    /// This register is the only register at offset 0x00 in the MCU_MISC block;
    /// there is no dedicated hardware CPU-ID register in this peripheral.
    pub fn get_cpu_id(&self) -> u32 {
        self.regs().mcu_bus_cfg0().read().bits()
    }

    /// Read the E907 RTC configuration register (`MCU_E907_RTC`, offset 0x14).
    ///
    /// Controls the E907 core real-time clock divider.  Reading this register
    /// reflects the current clock-divider setting, which can be used to infer
    /// the peripheral clock configuration.
    pub fn get_peripheral_clock_status(&self) -> u32 {
        self.regs().mcu_e907_rtc().read().bits()
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::mcu_misc::RegisterBlock;

    /// `get_error_status` reads `MCU1_LOG1` at offset 0x110 (word index 0x44).
    #[test]
    fn test_misc_error_logging_read() {
        let mut mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_MISC_PTR.with(|p| p.set(mock_ptr));

        // MCU1_LOG1 is at offset 0x110 from RegisterBlock base.
        unsafe {
            core::ptr::write_volatile(mock_data.as_mut_ptr().add(0x110 / 4), 0xDEADBEEF);
        }

        let misc = unsafe { PacMisc::steal() };
        let driver = McuMisc::new(misc);

        let status = driver.get_error_status();
        assert_eq!(status, 0xDEADBEEF, "get_error_status should read MCU1_LOG1 at offset 0x110");
    }

    /// `get_cpu_id` reads `MCU_BUS_CFG0` at offset 0x00 (word index 0).
    #[test]
    fn test_misc_cpu_id_read() {
        let mut mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_MISC_PTR.with(|p| p.set(mock_ptr));

        // MCU_BUS_CFG0 is at offset 0x00.
        unsafe {
            core::ptr::write_volatile(mock_data.as_mut_ptr(), 0xC906);
        }

        let misc = unsafe { PacMisc::steal() };
        let driver = McuMisc::new(misc);

        let id = driver.get_cpu_id();
        assert_eq!(id, 0xC906, "get_cpu_id should read MCU_BUS_CFG0 at offset 0x00");
    }

    /// `get_peripheral_clock_status` reads `MCU_E907_RTC` at offset 0x14 (word index 5).
    #[test]
    fn test_misc_peripheral_clock_status() {
        let mut mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_MISC_PTR.with(|p| p.set(mock_ptr));

        // MCU_E907_RTC is at offset 0x14 from RegisterBlock base.
        unsafe {
            core::ptr::write_volatile(mock_data.as_mut_ptr().add(0x14 / 4), 0x01);
        }

        let misc = unsafe { PacMisc::steal() };
        let driver = McuMisc::new(misc);

        let status = driver.get_peripheral_clock_status();
        assert_eq!(status, 0x01, "get_peripheral_clock_status should read MCU_E907_RTC at offset 0x14");
    }
}
