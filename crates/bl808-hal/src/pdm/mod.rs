//! Pulse Density Modulation (PDM) Microphone Driver
//!
//! The PDM peripheral is at base `0x2000_A800` on BL808. It digitises
//! PDM-output MEMS microphones and provides 16-bit PCM samples via a FIFO.
//!
//! # Register map (no dedicated PAC type)
//!
//! | Offset | Name            | Description                                           |
//! |--------|-----------------|-------------------------------------------------------|
//! | 0x00   | pdm_config      | [0]:en [2:1]:ch_sel [7:4]:decimation_ratio            |
//! | 0x04   | pdm_fifo_status | [5:0]:level [6]:empty [7]:full                        |
//! | 0x08   | pdm_fifo_data   | [15:0]: signed 16-bit PCM sample (FIFO pop on read)   |

#[cfg(any(test, feature = "mock"))]
extern crate std;

/// Base address of the PDM peripheral on BL808.
#[cfg(not(any(test, feature = "mock")))]
const PDM_BASE: usize = 0x2000_A800;

// Register offsets (in bytes).
const OFF_CONFIG: usize = 0x00;
const OFF_FIFO_STATUS: usize = 0x04;
const OFF_FIFO_DATA: usize = 0x08;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_PDM_PTR: core::cell::Cell<*mut u32> =
        const { core::cell::Cell::new(core::ptr::null_mut()) };
}

/// PDM channel selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdmChannel {
    /// No channel enabled.
    None = 0b00,
    /// Left channel only.
    Left = 0b01,
    /// Right channel only.
    Right = 0b10,
    /// Both left and right channels (stereo).
    Both = 0b11,
}

/// PDM driver.
pub struct PdmDriver {
    base: usize,
}

impl PdmDriver {
    /// Construct a new PDM driver.
    pub fn new() -> Self {
        #[cfg(not(any(test, feature = "mock")))]
        let base = PDM_BASE;
        #[cfg(any(test, feature = "mock"))]
        let base = MOCK_PDM_PTR.with(|p| p.get()) as usize;
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

    /// Enable PDM capture with `channels` and the given `decimation` ratio.
    ///
    /// `decimation` is stored as a 4-bit value in bits 7:4 of `pdm_config`.
    /// The actual decimation factor applied by hardware is `(decimation + 1) × 8`.
    pub fn enable(&mut self, channels: PdmChannel, decimation: u8) {
        let ch = (channels as u32) & 0x03;
        let dec = (decimation as u32) & 0x0F;
        self.modify(OFF_CONFIG, |v| {
            // Clear fields then write
            (v & !(0xF0 | 0x06 | 0x01)) | 0x01 | (ch << 1) | (dec << 4)
        });
    }

    /// Disable PDM capture.
    pub fn disable(&mut self) {
        self.modify(OFF_CONFIG, |v| v & !1);
    }

    /// Returns `true` when the RX FIFO contains at least one sample.
    pub fn is_fifo_ready(&self) -> bool {
        // FIFO is ready (not empty) when bit 6 (empty flag) is clear.
        self.read(OFF_FIFO_STATUS) & (1 << 6) == 0
    }

    /// Returns `true` when the RX FIFO is full.
    pub fn is_fifo_full(&self) -> bool {
        self.read(OFF_FIFO_STATUS) & (1 << 7) != 0
    }

    /// Returns the number of samples currently in the FIFO.
    pub fn fifo_level(&self) -> u8 {
        (self.read(OFF_FIFO_STATUS) & 0x3F) as u8
    }

    /// Pop one 16-bit signed PCM sample from the FIFO, or `None` if empty.
    pub fn read_sample(&self) -> Option<i16> {
        if !self.is_fifo_ready() {
            return None;
        }
        Some(self.read(OFF_FIFO_DATA) as u16 as i16)
    }
}

impl Default for PdmDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    fn make_driver() -> PdmDriver {
        let mock: &mut [u32; 16] = Box::leak(Box::new([0u32; 16]));
        MOCK_PDM_PTR.with(|p| p.set(mock.as_mut_ptr()));
        PdmDriver::new()
    }

    fn mock_ptr() -> *mut u32 {
        MOCK_PDM_PTR.with(|p| p.get())
    }

    fn rd(offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(mock_ptr().add(offset / 4)) }
    }

    fn wr(offset: usize, val: u32) {
        unsafe { core::ptr::write_volatile(mock_ptr().add(offset / 4), val) }
    }

    #[test]
    fn test_pdm_enable_sets_en_bit() {
        let mut d = make_driver();
        d.enable(PdmChannel::Left, 0);
        assert_ne!(rd(OFF_CONFIG) & 1, 0, "en bit must be set");
    }

    #[test]
    fn test_pdm_enable_channel_left() {
        let mut d = make_driver();
        d.enable(PdmChannel::Left, 0);
        assert_eq!((rd(OFF_CONFIG) >> 1) & 0x03, PdmChannel::Left as u32);
    }

    #[test]
    fn test_pdm_enable_channel_both() {
        let mut d = make_driver();
        d.enable(PdmChannel::Both, 3);
        assert_eq!((rd(OFF_CONFIG) >> 1) & 0x03, PdmChannel::Both as u32);
        assert_eq!((rd(OFF_CONFIG) >> 4) & 0x0F, 3u32);
    }

    #[test]
    fn test_pdm_disable_clears_en_bit() {
        let mut d = make_driver();
        d.enable(PdmChannel::Both, 0);
        d.disable();
        assert_eq!(rd(OFF_CONFIG) & 1, 0, "en bit must be cleared");
    }

    #[test]
    fn test_pdm_fifo_ready_when_not_empty() {
        let d = make_driver();
        // Empty flag (bit 6) clear → FIFO has data
        wr(OFF_FIFO_STATUS, 0x00);
        assert!(d.is_fifo_ready());
    }

    #[test]
    fn test_pdm_fifo_not_ready_when_empty() {
        let d = make_driver();
        wr(OFF_FIFO_STATUS, 1 << 6); // empty flag set
        assert!(!d.is_fifo_ready());
    }

    #[test]
    fn test_pdm_fifo_full() {
        let d = make_driver();
        wr(OFF_FIFO_STATUS, 1 << 7);
        assert!(d.is_fifo_full());
    }

    #[test]
    fn test_pdm_fifo_level() {
        let d = make_driver();
        wr(OFF_FIFO_STATUS, 17);
        assert_eq!(d.fifo_level(), 17);
    }

    #[test]
    fn test_pdm_read_sample_some() {
        let d = make_driver();
        wr(OFF_FIFO_STATUS, 0x00); // not empty
        wr(OFF_FIFO_DATA, 0x1234);
        assert_eq!(d.read_sample(), Some(0x1234i16));
    }

    #[test]
    fn test_pdm_read_sample_negative() {
        let d = make_driver();
        wr(OFF_FIFO_STATUS, 0x00);
        wr(OFF_FIFO_DATA, 0xFFFF_8000); // -32768 as i16
        assert_eq!(d.read_sample(), Some(i16::MIN));
    }

    #[test]
    fn test_pdm_read_sample_none_when_empty() {
        let d = make_driver();
        wr(OFF_FIFO_STATUS, 1 << 6); // empty
        assert_eq!(d.read_sample(), None);
    }
}
