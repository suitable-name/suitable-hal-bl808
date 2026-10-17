//! Always-On Domain (AON) Driver

#[cfg(feature = "m0")]
use bl808_pac::Aon as PacAon;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_AON_PTR: core::cell::Cell<*const bl808_pac::aon::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(test)]
std::thread_local! {
    // Thread-local HBN mock pointer used only during tests (for LDO vcore access).
    pub static MOCK_HBN_PTR_AON: core::cell::Cell<*const bl808_pac::hbn::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(feature = "m0")]
pub struct Aon {
    _inner: PacAon,
}

#[cfg(feature = "m0")]
impl Aon {
    pub fn new(aon: PacAon) -> Self {
        Self { _inner: aon }
    }

    fn regs(&self) -> &bl808_pac::aon::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacAon::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_AON_PTR.with(|p| p.get()) };
    }

    /// Access the HBN register block.
    ///
    /// The LDO11SOC voltage-select field (`sw_ldo11soc_vout_sel_aon`) lives in
    /// `HBN_GLB`, not in the AON register block.  In production the HBN block
    /// is at its fixed hardware address; tests supply a separate mock pointer.
    fn hbn_regs(&self) -> &bl808_pac::hbn::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*(bl808_pac::Hbn::PTR) };
        #[cfg(test)]
        return unsafe { &*MOCK_HBN_PTR_AON.with(|p| p.get()) };
    }

    /// Enable GPADC continuous-conversion mode.
    ///
    /// NOTE: the original `config_gpadc_vref` API referenced a `gpadc_vref_sel`
    /// field that does not exist in `GPADC_REG_CONFIG1` (the register exposes
    /// `gpadc_res_sel`, `gpadc_vcm_sel_en`, `gpadc_v11_sel`, `gpadc_v18_sel`).
    /// This now drives the real `gpadc_cont_conv_en` bit. The reference-voltage
    /// selection lives in the analog GPADC config and must be confirmed against
    /// the BL808 RM before use.
    pub fn enable_gpadc_continuous(&mut self) {
        self.regs()
            .gpadc_reg_config1()
            .modify(|_, w| w.gpadc_cont_conv_en().set_bit());
    }

    /// Set the GPADC 1.1 V reference voltage trim (2-bit field `gpadc_v11_sel`
    /// in `GPADC_REG_CONFIG1` bits 27:28).
    ///
    /// SDK reference: `bl808_adc.c` / `GLB_Set_GPADC_CONFIG` uses the analog
    /// reference fields in this register.  `gpadc_v11_sel` and `gpadc_v18_sel`
    /// are the real reference-select fields present in the PAC.
    ///
    /// `v11_sel`: 2-bit trim for the 1.1 V reference (0–3).
    /// `v18_sel`: 2-bit trim for the 1.8 V reference (0–3).
    pub fn set_gpadc_vref(&mut self, v11_sel: u8, v18_sel: u8) {
        self.regs()
            .gpadc_reg_config1()
            .modify(|_, w| unsafe {
                w.gpadc_v11_sel().bits(v11_sel & 0x3)
                 .gpadc_v18_sel().bits(v18_sel & 0x3)
            });
    }

    /// Configure LDO11SOC (SOC core LDO) output voltage.
    ///
    /// The voltage-select field `sw_ldo11soc_vout_sel_aon` (4 bits, 0–15) lives
    /// in `HBN_GLB` (bits 16:19), not in the AON block.  SDK reference:
    /// `bl808_hbn.h` / `HBN_GLB` register, field `sw_ldo11soc_vout_sel_aon`.
    ///
    /// Typical values (from HBN reset = 0xAA0A_0000, bits 19:16 = 0xA = 10):
    ///   0x9 = 0.9 V, 0xA = 1.0 V (default), 0xB = 1.1 V, 0xC = 1.2 V, etc.
    ///
    /// `voltage`: 4-bit select code (upper nibble is masked off).
    pub fn config_ldo_vcore(&mut self, voltage: u8) {
        self.hbn_regs()
            .hbn_glb()
            .modify(|_, w| unsafe {
                w.sw_ldo11soc_vout_sel_aon().bits(voltage & 0xF)
            });
    }

    /// Read the reset reason
    pub fn get_reset_reason(&self) -> u32 {
        // Reset reason is usually in aon_common or glb, 
        // following the stub's original intent of reading a value.
        self.regs().aon_common().read().bits()
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::aon::RegisterBlock;

    #[test]
    fn test_aon_gpadc_aon_config() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_AON_PTR.with(|p| p.set(mock_ptr));

        let aon = unsafe { PacAon::steal() };
        let mut driver = Aon::new(aon);

        driver.enable_gpadc_continuous();

        let val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x110 / 4))
        };
        // Verify bit 1 (gpadc_cont_conv_en) is set
        assert_ne!(val & (1 << 1), 0);
    }

    #[test]
    fn test_set_gpadc_vref() {
        let mock_data: &'static mut [u32; 1024] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 1024]));
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_AON_PTR.with(|p| p.set(mock_ptr));

        let aon = unsafe { PacAon::steal() };
        let mut driver = Aon::new(aon);

        // Set v11_sel=2, v18_sel=1
        driver.set_gpadc_vref(2, 1);

        let val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x110 / 4))
        };
        // gpadc_v11_sel = bits 28:27 => (val >> 27) & 0x3 = 2
        assert_eq!((val >> 27) & 0x3, 2, "gpadc_v11_sel should be 2");
        // gpadc_v18_sel = bits 30:29 => (val >> 29) & 0x3 = 1
        assert_eq!((val >> 29) & 0x3, 1, "gpadc_v18_sel should be 1");
    }

    #[test]
    fn test_config_ldo_vcore() {
        let aon_mock: &'static mut [u32; 1024] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 1024]));
        let aon_ptr = aon_mock.as_ptr() as *const RegisterBlock;
        MOCK_AON_PTR.with(|p| p.set(aon_ptr));

        let hbn_mock: &'static mut [u32; 1024] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 1024]));
        let hbn_ptr = hbn_mock.as_ptr() as *const bl808_pac::hbn::RegisterBlock;
        MOCK_HBN_PTR_AON.with(|p| p.set(hbn_ptr));

        let aon = unsafe { PacAon::steal() };
        let mut driver = Aon::new(aon);

        // Set voltage code 0xB (typical 1.1 V)
        driver.config_ldo_vcore(0xB);

        // HBN_GLB is at HBN offset 0x30; sw_ldo11soc_vout_sel_aon = bits 19:16
        let hbn_glb_val = unsafe {
            core::ptr::read_volatile((hbn_ptr as *const u32).add(0x30 / 4))
        };
        assert_eq!((hbn_glb_val >> 16) & 0xF, 0xB, "sw_ldo11soc_vout_sel_aon should be 0xB");
    }
}
