//! Camera Control Interface (CCI) Driver
//!
//! CCI is a hardware bus master that performs memory-mapped register access to
//! external camera sensors via a serial protocol (I2C/SCCB-compatible).
//!
//! Register layout (M0 PAC `Cci` peripheral):
//! - `cci_cfg`   (0x00): enable, SCLK enable/div, routing
//! - `cci_addr`  (0x04): 32-bit target address
//! - `cci_wdata` (0x08): 32-bit write data
//! - `cci_rdata` (0x0C): 32-bit read data (RO)
//! - `cci_ctl`   (0x10): write/read busy flags (RO)

#[cfg(feature = "m0")]
use bl808_pac::Cci as PacCci;

#[cfg(test)]
extern crate std;

#[cfg(all(test, feature = "m0"))]
std::thread_local! {
    pub static MOCK_CCI_PTR: core::cell::Cell<*const bl808_pac::cci::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// CCI bus master driver.
#[cfg(feature = "m0")]
pub struct Cci {
    _inner: PacCci,
}

#[cfg(feature = "m0")]
impl Cci {
    /// Construct a [`Cci`] driver from the PAC peripheral token.
    pub fn new(cci: PacCci) -> Self {
        Self { _inner: cci }
    }

    fn regs(&self) -> &bl808_pac::cci::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacCci::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_CCI_PTR.with(|p| p.get()) };
    }

    /// Enable the CCI module and master SCLK.
    pub fn enable(&mut self) {
        self.regs()
            .cci_cfg()
            .modify(|_, w| w.cci_en().set_bit().reg_m_cci_sclk_en().set_bit());
    }

    /// Disable the CCI module and master SCLK.
    pub fn disable(&mut self) {
        self.regs()
            .cci_cfg()
            .modify(|_, w| w.cci_en().clear_bit().reg_m_cci_sclk_en().clear_bit());
    }

    /// Set master SCLK clock division ratio (0–3, encoded as 2-bit field).
    pub fn set_clock_div(&mut self, div: u8) {
        self.regs()
            .cci_cfg()
            .modify(|_, w| unsafe { w.reg_div_m_cci_sclk().bits(div & 0x03) });
    }

    /// Write `data` to `addr` over the CCI bus.
    ///
    /// Polls `cci_ctl.cci_write_flag` until the hardware completes the
    /// transaction.
    pub fn write_reg(&mut self, addr: u32, data: u32) {
        let regs = self.regs();
        regs.cci_addr()
            .write(|w| unsafe { w.apb_cci_addr().bits(addr) });
        regs.cci_wdata()
            .write(|w| unsafe { w.apb_cci_wdata().bits(data) });
        while regs.cci_ctl().read().cci_write_flag().bit_is_set() {
            core::hint::spin_loop();
        }
    }

    /// Initiate a read from `addr` over the CCI bus and return the result.
    ///
    /// Polls `cci_ctl.cci_read_flag` until the hardware completes the
    /// transaction, then returns the value from `cci_rdata`.
    pub fn read_reg(&self, addr: u32) -> u32 {
        let regs = self.regs();
        regs.cci_addr()
            .write(|w| unsafe { w.apb_cci_addr().bits(addr) });
        while regs.cci_ctl().read().cci_read_flag().bit_is_set() {
            core::hint::spin_loop();
        }
        regs.cci_rdata().read().apb_cci_rdata().bits()
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::{cci::RegisterBlock, Cci as PacCci};

    #[test]
    fn test_cci_enable_sets_bits() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_CCI_PTR.with(|p| p.set(mock_ptr));

        let cci = unsafe { PacCci::steal() };
        let mut driver = Cci::new(cci);
        driver.enable();

        // cci_cfg at offset 0x00 → word index 0
        let val = unsafe { core::ptr::read_volatile(mock_ptr as *const u32) };
        assert_ne!(val & 1, 0, "cci_en (bit 0) must be set");
        assert_ne!(val & (1 << 4), 0, "reg_m_cci_sclk_en (bit 4) must be set");
    }

    #[test]
    fn test_cci_disable_clears_bits() {
        // Initialise with both bits set.
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        unsafe {
            core::ptr::write_volatile(mock_ptr as *mut u32, 0x0011); // cci_en + sclk_en
        }
        MOCK_CCI_PTR.with(|p| p.set(mock_ptr));

        let cci = unsafe { PacCci::steal() };
        let mut driver = Cci::new(cci);
        driver.disable();

        let val = unsafe { core::ptr::read_volatile(mock_ptr as *const u32) };
        assert_eq!(val & 1, 0, "cci_en must be cleared");
        assert_eq!(val & (1 << 4), 0, "reg_m_cci_sclk_en must be cleared");
    }

    #[test]
    fn test_cci_set_clock_div() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_CCI_PTR.with(|p| p.set(mock_ptr));

        let cci = unsafe { PacCci::steal() };
        let mut driver = Cci::new(cci);
        driver.set_clock_div(2);

        // reg_div_m_cci_sclk occupies bits 5:6 of cci_cfg
        let val = unsafe { core::ptr::read_volatile(mock_ptr as *const u32) };
        assert_eq!((val >> 5) & 0x03, 2, "reg_div_m_cci_sclk should be 2");
    }

    #[test]
    fn test_cci_write_reg_sets_addr_and_wdata() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_CCI_PTR.with(|p| p.set(mock_ptr));

        let cci = unsafe { PacCci::steal() };
        let mut driver = Cci::new(cci);
        // cci_ctl (offset 0x10 = word 4) is zero in mock → cci_write_flag cleared
        // → no spin, immediate return.
        driver.write_reg(0xDEAD_BEEF, 0x1234_5678);

        // cci_addr at offset 0x04 → word 1
        let addr_val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(1))
        };
        // cci_wdata at offset 0x08 → word 2
        let wdata_val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(2))
        };
        assert_eq!(addr_val, 0xDEAD_BEEF, "cci_addr mismatch");
        assert_eq!(wdata_val, 0x1234_5678, "cci_wdata mismatch");
    }

    #[test]
    fn test_cci_read_reg_sets_addr_and_returns_rdata() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        // Pre-set cci_rdata at offset 0x0C (word 3) with expected value.
        unsafe {
            core::ptr::write_volatile((mock_ptr as *mut u32).add(3), 0xABCD_1234);
        }
        MOCK_CCI_PTR.with(|p| p.set(mock_ptr));

        let cci = unsafe { PacCci::steal() };
        let driver = Cci::new(cci);
        // cci_ctl word 4 = 0 → cci_read_flag clear → immediate return.
        let result = driver.read_reg(0x0000_0100);

        let addr_val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(1))
        };
        assert_eq!(addr_val, 0x0000_0100, "cci_addr mismatch");
        assert_eq!(result, 0xABCD_1234, "cci_rdata mismatch");
    }
}
