//! ISP (Image Signal Processor) driver for the BL808 D0 core.
//!
//! The ISP pipeline processes raw Bayer data from the CSI/DVP receivers before
//! display or encoding.  There is no PAC peripheral type for this block;
//! register access is performed via raw MMIO.
//!
//! Hardware base: 0x3001_4000 (D0 multimedia domain).

#[cfg(test)]
extern crate std;

// Register offsets (byte-addressed, 32-bit registers).
const OFF_CTRL: usize = 0x00; // [0]:global_en  [1]:pipeline_en  [2]:bypass
const OFF_INPUT: usize = 0x04; // [3:0]:bayer_order  [7:4]:bit_depth
const OFF_BLC: usize = 0x08; // [11:0]:black_level_offset (12-bit, applied before demosaic)
const OFF_AE_CTRL: usize = 0x0c; // [0]:ae_en  [7:1]:ae_target
const OFF_STATUS: usize = 0x10; // [0]:busy  [1]:frame_done  (RO)

#[cfg(not(test))]
const ISP_BASE: usize = 0x3001_4000;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_ISP_PTR: core::cell::Cell<*mut u32> =
        const { core::cell::Cell::new(core::ptr::null_mut()) };
}

/// Bayer colour-filter array pattern (top-left 2×2 colour order).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BayerOrder {
    Rggb = 0,
    Bggr = 1,
    Grbg = 2,
    Gbrg = 3,
}

/// Raw ISP pipeline driver.
pub struct IspDriver {
    base: usize,
}

impl IspDriver {
    pub fn new() -> Self {
        #[cfg(not(test))]
        let base = ISP_BASE;
        #[cfg(test)]
        let base = MOCK_ISP_PTR.with(|p| p.get() as usize);
        Self { base }
    }

    #[inline]
    fn read(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base + offset) as *const u32) }
    }

    #[inline]
    fn write(&self, offset: usize, val: u32) {
        unsafe { core::ptr::write_volatile((self.base + offset) as *mut u32, val) }
    }

    #[inline]
    fn modify(&self, offset: usize, f: impl FnOnce(u32) -> u32) {
        self.write(offset, f(self.read(offset)));
    }

    /// Enable the global ISP clock gate.
    pub fn enable(&mut self) {
        self.modify(OFF_CTRL, |v| v | (1 << 0));
    }

    /// Disable the global ISP clock gate.
    pub fn disable(&mut self) {
        self.modify(OFF_CTRL, |v| v & !(1 << 0));
    }

    /// Enable the processing pipeline.
    pub fn enable_pipeline(&mut self) {
        self.modify(OFF_CTRL, |v| v | (1 << 1));
    }

    /// Disable the processing pipeline (keeps clock running).
    pub fn disable_pipeline(&mut self) {
        self.modify(OFF_CTRL, |v| v & !(1 << 1));
    }

    /// Bypass the ISP pipeline (raw pass-through mode).
    pub fn set_bypass(&mut self, bypass: bool) {
        self.modify(OFF_CTRL, |v| {
            if bypass { v | (1 << 2) } else { v & !(1 << 2) }
        });
    }

    /// Set Bayer CFA pattern of the connected sensor.
    pub fn set_bayer_order(&mut self, order: BayerOrder) {
        self.modify(OFF_INPUT, |v| (v & !0xf) | (order as u32 & 0xf));
    }

    /// Set raw input bit depth (e.g. 8, 10, 12).  Only bits [7:4] are written.
    pub fn set_raw_format(&mut self, format: u8) {
        self.modify(OFF_INPUT, |v| (v & !0xf) | (format as u32 & 0xf));
    }

    /// Set black-level correction offset (12-bit unsigned subtracted from each pixel).
    pub fn set_black_level(&mut self, offset: u16) {
        self.modify(OFF_BLC, |v| (v & !0xffff) | (offset as u32 & 0xffff));
    }

    /// Enable the auto-exposure controller.
    pub fn enable_ae(&mut self) {
        self.modify(OFF_AE_CTRL, |v| v | 1);
    }

    /// Disable the auto-exposure controller.
    pub fn disable_ae(&mut self) {
        self.modify(OFF_AE_CTRL, |v| v & !1);
    }

    /// Set AE target luminance (0–127, stored in bits [7:1]).
    pub fn set_ae_target(&mut self, luma: u8) {
        self.modify(OFF_AE_CTRL, |v| {
            (v & !0xfe) | (((luma & 0x7f) as u32) << 1)
        });
    }

    /// Returns `true` while the ISP is actively processing a frame.
    pub fn is_busy(&self) -> bool {
        self.read(OFF_STATUS) & 1 != 0
    }

    /// Returns `true` when frame processing has completed (clears on read).
    pub fn is_frame_done(&self) -> bool {
        self.read(OFF_STATUS) & (1 << 1) != 0
    }
}

impl Default for IspDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    fn make_driver() -> IspDriver {
        let buf = Box::leak(Box::new([0u32; 16]));
        MOCK_ISP_PTR.with(|p| p.set(buf.as_mut_ptr()));
        IspDriver::new()
    }

    #[test]
    fn test_isp_global_enable_disable() {
        let mut drv = make_driver();
        drv.enable();
        assert_ne!(drv.read(OFF_CTRL) & 1, 0, "global enable bit must be set");
        drv.disable();
        assert_eq!(drv.read(OFF_CTRL) & 1, 0, "global enable bit must be clear");
    }

    #[test]
    fn test_isp_pipeline_enable() {
        let mut drv = make_driver();
        drv.enable_pipeline();
        assert_ne!(drv.read(OFF_CTRL) & (1 << 1), 0);
        drv.disable_pipeline();
        assert_eq!(drv.read(OFF_CTRL) & (1 << 1), 0);
    }

    #[test]
    fn test_isp_bypass() {
        let mut drv = make_driver();
        drv.set_bypass(true);
        assert_ne!(drv.read(OFF_CTRL) & (1 << 2), 0);
        drv.set_bypass(false);
        assert_eq!(drv.read(OFF_CTRL) & (1 << 2), 0);
    }

    #[test]
    fn test_isp_raw_input_format() {
        let mut drv = make_driver();
        drv.set_raw_format(0xA);
        assert_eq!(drv.read(OFF_INPUT) & 0xf, 0xA);
    }

    #[test]
    fn test_isp_black_level_offset() {
        let mut drv = make_driver();
        drv.set_black_level(0x1234);
        assert_eq!(drv.read(OFF_BLC) & 0xffff, 0x1234);
    }

    #[test]
    fn test_isp_ae_target() {
        let mut drv = make_driver();
        drv.enable_ae();
        drv.set_ae_target(64);
        let v = drv.read(OFF_AE_CTRL);
        assert_ne!(v & 1, 0, "AE enable bit");
        assert_eq!((v >> 1) & 0x7f, 64, "AE target");
    }
}
