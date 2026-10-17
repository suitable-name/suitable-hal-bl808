//! Codec Miscellaneous Control Driver
//!
//! PAC-backed driver for the BL808 Codec subsystem.
//!
//! Registers in the Codec RegisterBlock (base 0x3002_0000):
//!   0x00 - codec_bus_ctrl  (rg_pclk_force_on[15:0])
//!   0x04 - codec_qos_ctrl  (per-client QoS bits)

use bl808_pac::Codec as PacCodec;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_CODEC_PTR: core::cell::Cell<*const bl808_pac::codec::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

/// Bitmask for `rg_pclk_force_on`: enable all clocks in the codec subsystem.
pub const PCLK_ALL: u16 = 0xFFFF;

pub struct Codec {
    _inner: PacCodec,
}

impl Codec {
    pub fn new(codec: PacCodec) -> Self {
        Self { _inner: codec }
    }

    fn regs(&self) -> &bl808_pac::codec::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacCodec::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_CODEC_PTR.with(|p| p.get()) };
    }

    /// Force the peripheral clocks on for selected codec subsystem components.
    ///
    /// `mask` selects which clocks to enable (see `PCLK_ALL` to enable all).
    pub fn force_clocks_on(&mut self, mask: u16) {
        self.regs()
            .codec_bus_ctrl()
            .modify(|r, w| unsafe { w.rg_pclk_force_on().bits(r.rg_pclk_force_on().bits() | mask) });
    }

    /// Force selected peripheral clocks off.
    pub fn force_clocks_off(&mut self, mask: u16) {
        self.regs()
            .codec_bus_ctrl()
            .modify(|r, w| unsafe { w.rg_pclk_force_on().bits(r.rg_pclk_force_on().bits() & !mask) });
    }

    /// Enable all codec peripheral clocks (set all `rg_pclk_force_on` bits).
    pub fn enable(&mut self) {
        self.regs()
            .codec_bus_ctrl()
            .write(|w| unsafe { w.rg_pclk_force_on().bits(PCLK_ALL) });
    }

    /// Disable all codec peripheral clocks.
    pub fn reset(&mut self) {
        self.regs()
            .codec_bus_ctrl()
            .write(|w| unsafe { w.rg_pclk_force_on().bits(0) });
    }

    /// Enable QoS for MJPEG encoder AXI read/write.
    pub fn set_jenc_qos(&mut self, aw: bool, ar: bool) {
        self.regs().codec_qos_ctrl().modify(|_, w| {
            w.reg_jenc_awqos()
                .bit(aw)
                .reg_jenc_arqos()
                .bit(ar)
        });
    }

    /// Enable QoS for H.264 video port 0 AXI read/write.
    pub fn set_vdo0_qos(&mut self, aw: bool, ar: bool) {
        self.regs().codec_qos_ctrl().modify(|_, w| {
            w.reg_vdo0_awqos()
                .bit(aw)
                .reg_vdo0_arqos()
                .bit(ar)
        });
    }

    /// Enable QoS for NPU/CNN AXI read/write.
    pub fn set_cnn_qos(&mut self, aw: bool, ar: bool) {
        self.regs().codec_qos_ctrl().modify(|_, w| {
            w.reg_cnn_awqos()
                .bit(aw)
                .reg_cnn_arqos()
                .bit(ar)
        });
    }
}

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use bl808_pac::codec::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_CODEC_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_codec_enable_sets_all_pclk_bits() {
        let regs = setup();
        let codec = unsafe { PacCodec::steal() };
        let mut driver = Codec::new(codec);

        driver.enable();

        assert_eq!(
            regs.codec_bus_ctrl().read().rg_pclk_force_on().bits(),
            0xFFFF,
            "all pclk bits should be set"
        );
    }

    #[test]
    fn test_codec_reset_clears_pclk_bits() {
        let regs = setup();
        // Pre-fill with all ones via raw write
        unsafe {
            core::ptr::write_volatile(regs.codec_bus_ctrl().as_ptr(), 0xFFFF_FFFFu32);
        }

        let codec = unsafe { PacCodec::steal() };
        let mut driver = Codec::new(codec);

        driver.reset();

        assert_eq!(
            regs.codec_bus_ctrl().read().rg_pclk_force_on().bits(),
            0,
            "pclk bits should be cleared"
        );
    }

    #[test]
    fn test_codec_set_cnn_qos() {
        let regs = setup();
        let codec = unsafe { PacCodec::steal() };
        let mut driver = Codec::new(codec);

        driver.set_cnn_qos(true, true);

        // reg_cnn_awqos = bit 10, reg_cnn_arqos = bit 11 (from PAC)
        let qos = regs.codec_qos_ctrl().read();
        assert!(qos.reg_cnn_awqos().bit_is_set(), "cnn awqos should be set");
        assert!(qos.reg_cnn_arqos().bit_is_set(), "cnn arqos should be set");
    }

    #[test]
    fn test_codec_initial_state() {
        let regs = setup();
        // Constructor should not write registers
        let codec = unsafe { PacCodec::steal() };
        let _driver = Codec::new(codec);

        assert_eq!(
            regs.codec_bus_ctrl().read().rg_pclk_force_on().bits(),
            0,
            "initial state should be 0"
        );
    }
}
