//! Multimedia GLB (MM_GLB) Driver
//!
//! MM_GLB controls the multimedia subsystem clocks and resets on the BL808
//! D0 (C906) core. It is mapped at `0x3001_2000`.
//!
//! # Register map (from PAC `bl808-d0`)
//!
//! | Offset | Name               | Description                                     |
//! |--------|--------------------|-------------------------------------------------|
//! | 0x00   | MM_CLK_CTRL_CPU    | MM CPU / PLL / bus clock enables and selectors  |
//! | 0x04   | MM_CLK_CPU         | D0 CPU and CNN clock dividers                   |
//! | 0x08   | DP_CLK             | Display-Port pixel clock divider and selector   |
//! | 0x0C   | CODEC_CLK          | H264 / codec clock divider enable and selector  |
//! | 0x10   | MM_CLK_CTRL_PERI   | MM peripheral clock control 0                   |
//! | 0x18   | MM_CLK_CTRL_PERI3  | MM peripheral clock control 1                   |
//! | 0x40   | MM_SW_SYS_RESET    | MM system / CPU0 software reset                 |
//! | 0x44   | SW_RESET_MM_PERI   | MM peripheral soft-reset                        |
//! | 0x48   | SW_RESET_SUB       | MM subsystem soft-reset                         |
//! | 0x4C   | SW_RESET_CODEC_SUB | Codec subsystem soft-reset                      |

#[cfg(feature = "d0")]
use bl808_pac::MmGlb as PacMmGlb;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_MMGLB_PTR: core::cell::Cell<*const bl808_pac::mm_glb::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

/// Multimedia GLB driver (D0 core only).
#[cfg(feature = "d0")]
pub struct MmGlb {
    _inner: PacMmGlb,
}

#[cfg(feature = "d0")]
impl MmGlb {
    /// Construct the MM_GLB driver from the PAC peripheral token.
    pub fn new(mmglb: PacMmGlb) -> Self {
        Self { _inner: mmglb }
    }

    fn regs(&self) -> &bl808_pac::mm_glb::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacMmGlb::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_MMGLB_PTR.with(|p| p.get()) };
    }

    /// Enable the H264/codec clock divider (`CODEC_CLK`, offset 0x0C, bit 8:
    /// `reg_h264_clk_div_en`).
    ///
    /// Must be called before starting H264 encode/decode or the codec DMA.
    pub fn enable_codec_clock(&mut self) {
        self.regs()
            .codec_clk()
            .modify(|_, w| w.reg_h264_clk_div_en().set_bit());
    }

    /// Assert the multimedia system software reset (`MM_SW_SYS_RESET`, offset
    /// 0x40, bit 0: `reg_ctrl_sys_reset`).
    ///
    /// This resets the entire MM subsystem. The caller is responsible for
    /// de-asserting the reset and re-initialising peripherals afterwards.
    pub fn trigger_mm_reset(&mut self) {
        self.regs()
            .mm_sw_sys_reset()
            .modify(|_, w| w.reg_ctrl_sys_reset().set_bit());
    }

    /// Enable the Display-Port pixel clock divider (`DP_CLK`, offset 0x08,
    /// bit 16: `reg_dp_clk_div_en`).
    ///
    /// Required before enabling the DSI transmitter or the MIPI D-PHY.
    pub fn enable_dsi_clock(&mut self) {
        self.regs()
            .dp_clk()
            .modify(|_, w| w.reg_dp_clk_div_en().set_bit());
    }
}

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use bl808_pac::mm_glb::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_MMGLB_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    /// `enable_codec_clock` must set `reg_h264_clk_div_en` (bit 8) in
    /// CODEC_CLK at PAC offset 0x0C.
    #[test]
    fn test_mmglb_mm_clock_enable() {
        let regs = setup();

        let mmglb = unsafe { PacMmGlb::steal() };
        let mut driver = MmGlb::new(mmglb);

        driver.enable_codec_clock();

        // CODEC_CLK is at offset 0x0C; reg_h264_clk_div_en is bit 8.
        let val = regs.codec_clk().read();
        assert!(
            val.reg_h264_clk_div_en().bit_is_set(),
            "reg_h264_clk_div_en (bit 8 of CODEC_CLK @ 0x0C) should be set"
        );
    }

    /// `trigger_mm_reset` must set `reg_ctrl_sys_reset` (bit 0) in
    /// MM_SW_SYS_RESET at PAC offset 0x40.
    #[test]
    fn test_mmglb_mm_reset_trigger() {
        let regs = setup();

        let mmglb = unsafe { PacMmGlb::steal() };
        let mut driver = MmGlb::new(mmglb);

        driver.trigger_mm_reset();

        // MM_SW_SYS_RESET is at offset 0x40; reg_ctrl_sys_reset is bit 0.
        let val = regs.mm_sw_sys_reset().read();
        assert!(
            val.reg_ctrl_sys_reset().bit_is_set(),
            "reg_ctrl_sys_reset (bit 0 of MM_SW_SYS_RESET @ 0x40) should be set"
        );
    }

    /// `enable_dsi_clock` must set `reg_dp_clk_div_en` (bit 16) in
    /// DP_CLK at PAC offset 0x08.
    #[test]
    fn test_mmglb_dsi_clock_enable() {
        let regs = setup();

        let mmglb = unsafe { PacMmGlb::steal() };
        let mut driver = MmGlb::new(mmglb);

        driver.enable_dsi_clock();

        // DP_CLK is at offset 0x08; reg_dp_clk_div_en is bit 16.
        let val = regs.dp_clk().read();
        assert!(
            val.reg_dp_clk_div_en().bit_is_set(),
            "reg_dp_clk_div_en (bit 16 of DP_CLK @ 0x08) should be set"
        );
    }
}
