//! On-Screen Display (OSD) Overlay Driver
//!
//! The OSD peripheral is in the BL808 multimedia subsystem at base
//! `0x3001_5000` (typically accessed from the D0 core). It composites up to
//! four independently-positioned ARGB overlay layers on top of a video stream.
//!
//! # Register map (no dedicated PAC type)
//!
//! | Offset          | Name         | Description                                     |
//! |-----------------|--------------|-------------------------------------------------|
//! | 0x00            | osd_ctrl     | [3:0]:layer_en  [4]:global_en  [7:5]:blend_mode |
//! | 0x10 + n×0x10   | layerN_pos   | [15:0]:x  [31:16]:y                             |
//! | 0x14 + n×0x10   | layerN_size  | [15:0]:width  [31:16]:height                    |
//! | 0x18 + n×0x10   | layerN_color | [31:0]: ARGB colour key (alpha in bits 31:24)   |
//! | 0x1C + n×0x10   | layerN_addr  | frame buffer base address                       |

#[cfg(test)]
extern crate std;

/// Base address of the OSD peripheral on BL808.
#[cfg(not(test))]
const OSD_BASE: usize = 0x3001_5000;

// Register offsets (in bytes).
const OFF_CTRL: usize = 0x00;

#[inline]
const fn layer_off_pos(n: usize) -> usize {
    0x10 + n * 0x10
}
#[inline]
const fn layer_off_size(n: usize) -> usize {
    0x14 + n * 0x10
}
#[inline]
const fn layer_off_color(n: usize) -> usize {
    0x18 + n * 0x10
}
#[inline]
const fn layer_off_addr(n: usize) -> usize {
    0x1C + n * 0x10
}

/// Maximum number of OSD layers supported by the hardware.
pub const OSD_LAYER_COUNT: usize = 4;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_OSD_PTR: core::cell::Cell<*mut u32> =
        const { core::cell::Cell::new(core::ptr::null_mut()) };
}

/// OSD alpha-blending mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdBlendMode {
    /// Replace: overlay completely replaces the video pixel.
    Replace = 0,
    /// Alpha-blend using the `A` channel in the ARGB colour word.
    Alpha = 1,
    /// Colour-key: pixels matching `layerN_color` are transparent.
    ColorKey = 2,
}

/// Configuration for a single OSD layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerConfig {
    /// Top-left x position in pixels.
    pub x: u16,
    /// Top-left y position in pixels.
    pub y: u16,
    /// Width of the overlay in pixels.
    pub width: u16,
    /// Height of the overlay in pixels.
    pub height: u16,
    /// ARGB colour word; bits 31:24 carry the alpha channel.
    pub color: u32,
    /// Physical address of the overlay frame buffer.
    pub buf_addr: u32,
}

/// OSD overlay driver.
pub struct OsdDriver {
    base: usize,
}

impl OsdDriver {
    /// Construct a new OSD driver.
    pub fn new() -> Self {
        #[cfg(not(test))]
        let base = OSD_BASE;
        #[cfg(test)]
        let base = MOCK_OSD_PTR.with(|p| p.get()) as usize;
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
    fn modify<F: FnOnce(u32) -> u32>(&self, offset: usize, f: F) {
        let old = self.read(offset);
        self.write(offset, f(old));
    }

    /// Enable the global OSD compositor.
    pub fn enable(&mut self) {
        self.modify(OFF_CTRL, |v| v | (1 << 4));
    }

    /// Disable the global OSD compositor.
    pub fn disable(&mut self) {
        self.modify(OFF_CTRL, |v| v & !(1 << 4));
    }

    /// Set the global blending mode applied to all layers.
    pub fn set_blend_mode(&mut self, mode: OsdBlendMode) {
        self.modify(OFF_CTRL, |v| (v & !(0x07 << 5)) | ((mode as u32) << 5));
    }

    /// Configure and enable a single OSD layer.
    ///
    /// # Panics
    ///
    /// Panics if `layer >= OSD_LAYER_COUNT`.
    pub fn configure_layer(&mut self, layer: usize, cfg: LayerConfig) {
        assert!(layer < OSD_LAYER_COUNT, "layer index out of range");
        self.write(
            layer_off_pos(layer),
            (cfg.x as u32) | ((cfg.y as u32) << 16),
        );
        self.write(
            layer_off_size(layer),
            (cfg.width as u32) | ((cfg.height as u32) << 16),
        );
        self.write(layer_off_color(layer), cfg.color);
        self.write(layer_off_addr(layer), cfg.buf_addr);
        // Set the per-layer enable bit.
        self.modify(OFF_CTRL, |v| v | (1u32 << layer));
    }

    /// Enable a previously configured layer (bit in `osd_ctrl`).
    pub fn enable_layer(&mut self, layer: usize) {
        assert!(layer < OSD_LAYER_COUNT);
        self.modify(OFF_CTRL, |v| v | (1u32 << layer));
    }

    /// Disable a layer (clear its bit in `osd_ctrl`).
    pub fn disable_layer(&mut self, layer: usize) {
        assert!(layer < OSD_LAYER_COUNT);
        self.modify(OFF_CTRL, |v| v & !(1u32 << layer));
    }

    /// Returns `true` if the given layer is currently enabled.
    pub fn is_layer_enabled(&self, layer: usize) -> bool {
        assert!(layer < OSD_LAYER_COUNT);
        self.read(OFF_CTRL) & (1u32 << layer) != 0
    }
}

impl Default for OsdDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    fn make_driver() -> OsdDriver {
        let mock: &mut [u32; 64] = Box::leak(Box::new([0u32; 64]));
        MOCK_OSD_PTR.with(|p| p.set(mock.as_mut_ptr()));
        OsdDriver::new()
    }

    fn mock_ptr() -> *mut u32 {
        MOCK_OSD_PTR.with(|p| p.get())
    }

    fn rd(offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(mock_ptr().add(offset / 4)) }
    }

    #[test]
    fn test_osd_global_enable() {
        let mut d = make_driver();
        d.enable();
        assert_ne!(rd(OFF_CTRL) & (1 << 4), 0);
    }

    #[test]
    fn test_osd_global_disable() {
        let mut d = make_driver();
        d.enable();
        d.disable();
        assert_eq!(rd(OFF_CTRL) & (1 << 4), 0);
    }

    #[test]
    fn test_osd_blend_mode_alpha() {
        let mut d = make_driver();
        d.set_blend_mode(OsdBlendMode::Alpha);
        assert_eq!((rd(OFF_CTRL) >> 5) & 0x07, OsdBlendMode::Alpha as u32);
    }

    #[test]
    fn test_osd_blend_mode_colorkey() {
        let mut d = make_driver();
        d.set_blend_mode(OsdBlendMode::ColorKey);
        assert_eq!((rd(OFF_CTRL) >> 5) & 0x07, OsdBlendMode::ColorKey as u32);
    }

    #[test]
    fn test_osd_configure_layer0() {
        let mut d = make_driver();
        d.configure_layer(0, LayerConfig { x: 10, y: 20, width: 320, height: 240, color: 0xFF_00_00_FF, buf_addr: 0x8000_0000 });

        // Position: x=10, y=20
        let pos = rd(layer_off_pos(0));
        assert_eq!(pos & 0xFFFF, 10);
        assert_eq!((pos >> 16) & 0xFFFF, 20);

        // Size: 320×240
        let size = rd(layer_off_size(0));
        assert_eq!(size & 0xFFFF, 320);
        assert_eq!((size >> 16) & 0xFFFF, 240);

        // Color
        assert_eq!(rd(layer_off_color(0)), 0xFF_00_00_FF);

        // Buffer address
        assert_eq!(rd(layer_off_addr(0)), 0x8000_0000);

        // Enable bit for layer 0
        assert_ne!(rd(OFF_CTRL) & 1, 0);
    }

    #[test]
    fn test_osd_configure_layer3() {
        let mut d = make_driver();
        d.configure_layer(3, LayerConfig { x: 0, y: 0, width: 100, height: 50, color: 0xAA_BB_CC_DD, buf_addr: 0x9000_0000 });
        assert_eq!(rd(layer_off_pos(3)), 0); // x=0, y=0
        assert_eq!(rd(layer_off_size(3)), 50 << 16 | 100);
        assert_ne!(rd(OFF_CTRL) & (1 << 3), 0, "layer 3 enable bit");
    }

    #[test]
    fn test_osd_enable_disable_layer() {
        let mut d = make_driver();
        d.enable_layer(2);
        assert!(d.is_layer_enabled(2));
        d.disable_layer(2);
        assert!(!d.is_layer_enabled(2));
    }

    #[test]
    fn test_osd_multiple_layers_independent() {
        let mut d = make_driver();
        d.enable_layer(0);
        d.enable_layer(3);
        assert!(d.is_layer_enabled(0));
        assert!(!d.is_layer_enabled(1));
        assert!(!d.is_layer_enabled(2));
        assert!(d.is_layer_enabled(3));
    }

    #[test]
    #[should_panic]
    fn test_osd_layer_out_of_range_panics() {
        let mut d = make_driver();
        d.enable_layer(OSD_LAYER_COUNT); // should panic
    }
}
