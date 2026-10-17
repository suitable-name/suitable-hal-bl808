//! RF Automatic Gain Control (AGC) Driver
//!
//! The AGC peripheral is part of the BL808 RF subsystem at base address
//! `0x2000_B000`. It controls the receive-path gain of the RF front-end.
//!
//! # Register map (no dedicated PAC type)
//!
//! | Offset | Name       | Description                                      |
//! |--------|------------|--------------------------------------------------|
//! | 0x00   | agc_config | [0]:en [1]:manual_mode [7:2]:manual_gain (6-bit) |
//! | 0x04   | agc_gain   | [7:0]: current gain index (RO in auto mode)      |
//! | 0x08   | agc_rssi   | [7:0]: RSSI reading (signed, dBm-relative)       |
//! | 0x0C   | agc_status | [0]:locked [1]:busy                              |

#[cfg(test)]
extern crate std;

/// Base address of the AGC peripheral on BL808.
#[cfg(not(test))]
const AGC_BASE: usize = 0x2000_B000;

// Register offsets (in bytes).
const OFF_CONFIG: usize = 0x00;
const OFF_GAIN: usize = 0x04;
const OFF_RSSI: usize = 0x08;
const OFF_STATUS: usize = 0x0C;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_AGC_PTR: core::cell::Cell<*mut u32> =
        const { core::cell::Cell::new(core::ptr::null_mut()) };
}

/// AGC operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcMode {
    /// Hardware automatically adjusts gain.
    Auto = 0,
    /// Software controls gain via [`AgcDriver::set_gain`].
    Manual = 1,
}

/// RF AGC driver.
pub struct AgcDriver {
    base: usize,
}

impl AgcDriver {
    /// Construct a new AGC driver.
    pub fn new() -> Self {
        #[cfg(not(test))]
        let base = AGC_BASE;
        #[cfg(test)]
        let base = MOCK_AGC_PTR.with(|p| p.get()) as usize;
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

    /// Enable the AGC module.
    pub fn enable(&mut self) {
        self.modify(OFF_CONFIG, |v| v | 1);
    }

    /// Disable the AGC module.
    pub fn disable(&mut self) {
        self.modify(OFF_CONFIG, |v| v & !1);
    }

    /// Set the AGC operating mode (auto or manual).
    pub fn set_mode(&mut self, mode: AgcMode) {
        self.modify(OFF_CONFIG, |v| match mode {
            AgcMode::Auto => v & !(1 << 1),
            AgcMode::Manual => v | (1 << 1),
        });
    }

    /// Set the manual gain index (0–63, 6-bit field).
    ///
    /// Only effective when [`AgcMode::Manual`] is active.
    pub fn set_gain(&mut self, gain: u8) {
        let gain6 = (gain & 0x3F) as u32;
        self.modify(OFF_CONFIG, |v| (v & !(0x3F << 2)) | (gain6 << 2));
    }

    /// Read the current hardware gain index (auto or manual).
    pub fn read_gain(&self) -> u8 {
        (self.read(OFF_GAIN) & 0xFF) as u8
    }

    /// Read the RF RSSI as a signed dBm-relative value.
    pub fn read_rssi(&self) -> i8 {
        self.read(OFF_RSSI) as u8 as i8
    }

    /// Returns `true` while the AGC is actively adjusting gain.
    pub fn is_busy(&self) -> bool {
        self.read(OFF_STATUS) & (1 << 1) != 0
    }

    /// Returns `true` when the AGC has locked onto a stable gain.
    pub fn is_locked(&self) -> bool {
        self.read(OFF_STATUS) & 1 != 0
    }
}

impl Default for AgcDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    // Heap-allocate the mock so the pointer stays valid after `make_driver` returns.
    fn make_driver() -> AgcDriver {
        let mock: &mut [u32; 16] = Box::leak(Box::new([0u32; 16]));
        MOCK_AGC_PTR.with(|p| p.set(mock.as_mut_ptr()));
        AgcDriver::new()
    }

    fn mock_ptr() -> *mut u32 {
        MOCK_AGC_PTR.with(|p| p.get())
    }

    fn rd(offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(mock_ptr().add(offset / 4)) }
    }

    fn wr(offset: usize, val: u32) {
        unsafe { core::ptr::write_volatile(mock_ptr().add(offset / 4), val) }
    }

    #[test]
    fn test_agc_enable_sets_bit0() {
        let mut d = make_driver();
        d.enable();
        assert_ne!(rd(OFF_CONFIG) & 1, 0, "enable bit must be set");
    }

    #[test]
    fn test_agc_disable_clears_bit0() {
        let mut d = make_driver();
        d.enable();
        d.disable();
        assert_eq!(rd(OFF_CONFIG) & 1, 0, "enable bit must be cleared");
    }

    #[test]
    fn test_agc_set_mode_manual() {
        let mut d = make_driver();
        d.set_mode(AgcMode::Manual);
        assert_ne!(rd(OFF_CONFIG) & (1 << 1), 0, "manual bit must be set");
    }

    #[test]
    fn test_agc_set_mode_auto_clears_manual_bit() {
        let mut d = make_driver();
        d.set_mode(AgcMode::Manual);
        d.set_mode(AgcMode::Auto);
        assert_eq!(rd(OFF_CONFIG) & (1 << 1), 0, "manual bit must be cleared");
    }

    #[test]
    fn test_agc_set_gain_field() {
        let mut d = make_driver();
        d.set_gain(42);
        assert_eq!((rd(OFF_CONFIG) >> 2) & 0x3F, 42, "gain field mismatch");
    }

    #[test]
    fn test_agc_set_gain_clamped_to_6bits() {
        let mut d = make_driver();
        d.set_gain(0xFF);
        assert_eq!((rd(OFF_CONFIG) >> 2) & 0x3F, 0x3F, "gain must be masked to 6 bits");
    }

    #[test]
    fn test_agc_read_gain() {
        let d = make_driver();
        wr(OFF_GAIN, 0x1B);
        assert_eq!(d.read_gain(), 0x1B);
    }

    #[test]
    fn test_agc_read_rssi_positive() {
        let d = make_driver();
        wr(OFF_RSSI, 42);
        assert_eq!(d.read_rssi(), 42i8);
    }

    #[test]
    fn test_agc_read_rssi_negative() {
        let d = make_driver();
        wr(OFF_RSSI, 0xD6); // -42 as i8
        assert_eq!(d.read_rssi(), -42i8);
    }

    #[test]
    fn test_agc_is_busy() {
        let d = make_driver();
        wr(OFF_STATUS, 1 << 1);
        assert!(d.is_busy());
        wr(OFF_STATUS, 0);
        assert!(!d.is_busy());
    }

    #[test]
    fn test_agc_is_locked() {
        let d = make_driver();
        wr(OFF_STATUS, 1);
        assert!(d.is_locked());
        wr(OFF_STATUS, 0);
        assert!(!d.is_locked());
    }
}
