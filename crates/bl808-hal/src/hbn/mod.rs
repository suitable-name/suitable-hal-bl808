//! Hibernate Domain (HBN) Driver
//!
//! Controls the BL808 HBN (Hibernate) block registers:
//! - `HBN_GLB` (offset 0x30): root clock selection, LDO voltages
//! - `HBN_CTL` (offset 0x00): RTC enable, power-down control
//! - `HBN_IRQ_MODE` (offset 0x14): wakeup-source pin mask and mode

#[cfg(feature = "m0")]
use bl808_pac::Hbn as PacHbn;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_HBN_PTR: core::cell::Cell<*const bl808_pac::hbn::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

/// HBN domain driver.
///
/// Only available when the `m0` feature is enabled.
#[cfg(feature = "m0")]
pub struct Hbn {
    _inner: PacHbn,
}

#[cfg(feature = "m0")]
impl Hbn {
    /// Construct an HBN driver from the PAC peripheral token.
    pub fn new(hbn: PacHbn) -> Self {
        Self { _inner: hbn }
    }

    fn regs(&self) -> &bl808_pac::hbn::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacHbn::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_HBN_PTR.with(|p| p.get()) };
    }

    /// Select the system root clock source via `HBN_GLB.hbn_root_clk_sel`.
    ///
    /// `sel` is a 2-bit value:
    /// - `0b00` — RC32M oscillator (reset default)
    /// - `0b01` — XTAL oscillator
    /// - `0b10` — PLL
    ///
    /// Writes `HBN_GLB` (offset 0x30) using the PAC accessor.
    pub fn set_root_clk_sel(&mut self, sel: u8) {
        self.regs()
            .hbn_glb()
            .modify(|_, w| unsafe { w.hbn_root_clk_sel().bits(sel & 0x3) });
    }

    /// Enable the HBN RTC counter.
    ///
    /// Sets bit 0 of `rtc_ctl` in `HBN_CTL` (offset 0x00).
    /// Bit 0 (`rtc_ctl[0]`) is the RTC enable bit per the BL808 reference manual.
    pub fn enable_rtc(&mut self) {
        self.regs()
            .hbn_ctl()
            .modify(|r, w| unsafe { w.rtc_ctl().bits(r.rtc_ctl().bits() | 1) });
    }

    /// Set the wakeup-source pin mask in `HBN_IRQ_MODE` (offset 0x14).
    ///
    /// `mask` is a 9-bit value written to the `hbn_pin_wakeup_mask` field
    /// (bits 12:4 of `HBN_IRQ_MODE`). Each bit enables the corresponding
    /// HBN wakeup pin.
    pub fn set_wakeup_pin_mask(&mut self, mask: u16) {
        self.regs()
            .hbn_irq_mode()
            .modify(|_, w| unsafe { w.hbn_pin_wakeup_mask().bits(mask & 0x1FF) });
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::hbn::RegisterBlock;

    #[test]
    fn test_hbn_set_root_clk_sel_pll() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_HBN_PTR.with(|p| p.set(mock_ptr));

        let hbn = unsafe { PacHbn::steal() };
        let mut driver = Hbn::new(hbn);

        driver.set_root_clk_sel(0b10); // select PLL

        // HBN_GLB is at offset 0x30 = word index 12; hbn_root_clk_sel = bits 1:0
        let val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x30 / 4))
        };
        assert_eq!(val & 0x3, 0b10, "hbn_root_clk_sel should be 0b10 (PLL)");
    }

    #[test]
    fn test_hbn_set_root_clk_sel_xtal() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_HBN_PTR.with(|p| p.set(mock_ptr));

        let hbn = unsafe { PacHbn::steal() };
        let mut driver = Hbn::new(hbn);

        driver.set_root_clk_sel(0b01); // select XTAL

        let val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x30 / 4))
        };
        assert_eq!(val & 0x3, 0b01, "hbn_root_clk_sel should be 0b01 (XTAL)");
    }

    #[test]
    fn test_hbn_enable_rtc_sets_bit0_of_hbn_ctl() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_HBN_PTR.with(|p| p.set(mock_ptr));

        let hbn = unsafe { PacHbn::steal() };
        let mut driver = Hbn::new(hbn);

        driver.enable_rtc();

        // HBN_CTL is at offset 0x00 = word index 0; rtc_ctl[0] = bit 0
        let val = unsafe {
            core::ptr::read_volatile(mock_ptr as *const u32)
        };
        assert_ne!(val & 1, 0, "rtc_ctl bit 0 (RTC enable) should be set in HBN_CTL");
    }

    #[test]
    fn test_hbn_set_wakeup_pin_mask() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_HBN_PTR.with(|p| p.set(mock_ptr));

        let hbn = unsafe { PacHbn::steal() };
        let mut driver = Hbn::new(hbn);

        driver.set_wakeup_pin_mask(0b1_0101_0101); // 9-bit mask

        // HBN_IRQ_MODE is at offset 0x14 = word index 5; hbn_pin_wakeup_mask = bits 12:4
        let val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x14 / 4))
        };
        assert_eq!((val >> 4) & 0x1FF, 0b1_0101_0101, "hbn_pin_wakeup_mask should be set");
    }

    #[test]
    fn test_hbn_mask_is_9bit_clamped() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_HBN_PTR.with(|p| p.set(mock_ptr));

        let hbn = unsafe { PacHbn::steal() };
        let mut driver = Hbn::new(hbn);

        // Pass a value wider than 9 bits — should be masked to 9 bits
        driver.set_wakeup_pin_mask(0xFFFF);

        let val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x14 / 4))
        };
        assert_eq!((val >> 4) & 0x1FF, 0x1FF, "mask should be clamped to 9 bits");
    }
}
