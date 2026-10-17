//! Temperature Sensor (TSEN) Driver
//!
//! TSEN is integrated in the BL808 AON (Always-On) subsystem at base
//! `0x2000_1000`. It provides a 12-bit raw ADC code that is converted to
//! Celsius using the formula:
//!
//! ```text
//! T_celsius = (raw as i32 - TSEN_OFFSET) * 10 / TSEN_GAIN_X10
//! ```
//!
//! The default calibration constants are approximations valid across the
//! typical BL808 operating range and may need field calibration for high
//! accuracy applications.
//!
//! # Register map (no dedicated PAC type)
//!
//! | Offset | Name        | Description                                              |
//! |--------|-------------|----------------------------------------------------------|
//! | 0x00   | tsen_config | [0]:en [1]:start (self-clearing) [2]:cont_en             |
//! | 0x04   | tsen_data   | [11:0]: raw 12-bit ADC output (valid when done)          |
//! | 0x08   | tsen_status | [0]:busy [1]:done                                        |

#[cfg(test)]
extern crate std;

/// Base address of the TSEN peripheral (AON subsystem).
#[cfg(not(test))]
const TSEN_BASE: usize = 0x2000_1000;

// Register offsets (in bytes).
const OFF_CONFIG: usize = 0x00;
const OFF_DATA: usize = 0x04;
const OFF_STATUS: usize = 0x08;

/// Approximate calibration offset for BL808 TSEN.
///
/// Derived from: raw ADC code ≈ 2048 at 25 °C (mid-scale of 12-bit ADC).
pub const TSEN_OFFSET: i32 = 2048;

/// Gain × 10 (to avoid floating-point): ≈ 7.26 °C per ADC LSB.
///
/// `gain_x10 = 73` means each code step ≈ 7.3 × 0.1 °C = 0.73 °C.
/// Adjust for real hardware after factory calibration.
pub const TSEN_GAIN_X10: i32 = 73;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_TSEN_PTR: core::cell::Cell<*mut u32> =
        const { core::cell::Cell::new(core::ptr::null_mut()) };
}

/// Temperature sensor driver.
pub struct TsenDriver {
    base: usize,
}

impl TsenDriver {
    /// Construct a new TSEN driver.
    pub fn new() -> Self {
        #[cfg(not(test))]
        let base = TSEN_BASE;
        #[cfg(test)]
        let base = MOCK_TSEN_PTR.with(|p| p.get()) as usize;
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

    /// Power on the temperature sensor.
    pub fn enable(&mut self) {
        self.modify(OFF_CONFIG, |v| v | 1);
    }

    /// Power off the temperature sensor.
    pub fn disable(&mut self) {
        self.modify(OFF_CONFIG, |v| v & !1);
    }

    /// Trigger a single one-shot conversion.
    ///
    /// The `start` bit is self-clearing. Poll [`is_busy`] or [`is_done`]
    /// before reading the result.
    pub fn start_conversion(&mut self) {
        self.modify(OFF_CONFIG, |v| v | (1 << 1));
    }

    /// Enable continuous conversion mode.
    pub fn enable_continuous(&mut self) {
        self.modify(OFF_CONFIG, |v| v | (1 << 2));
    }

    /// Disable continuous conversion mode.
    pub fn disable_continuous(&mut self) {
        self.modify(OFF_CONFIG, |v| v & !(1 << 2));
    }

    /// Returns `true` while a conversion is in progress.
    pub fn is_busy(&self) -> bool {
        self.read(OFF_STATUS) & 1 != 0
    }

    /// Returns `true` when a new result is ready.
    pub fn is_done(&self) -> bool {
        self.read(OFF_STATUS) & (1 << 1) != 0
    }

    /// Read the raw 12-bit ADC code from the last conversion.
    pub fn read_raw(&self) -> u16 {
        (self.read(OFF_DATA) & 0x0FFF) as u16
    }

    /// Convert the raw ADC code to a Celsius temperature (in tenths of a degree).
    ///
    /// Returns the temperature as `i16` in units of 0.1 °C
    /// (e.g., `253` means 25.3 °C).
    pub fn raw_to_decicelsius(raw: u16) -> i16 {
        ((raw as i32 - TSEN_OFFSET) * 10 / TSEN_GAIN_X10) as i16
    }

    /// Read and convert the temperature in tenths of a degree Celsius.
    ///
    /// Combines [`read_raw`] and [`raw_to_decicelsius`].
    pub fn read_decicelsius(&self) -> i16 {
        Self::raw_to_decicelsius(self.read_raw())
    }
}

impl Default for TsenDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    fn make_driver() -> TsenDriver {
        let mock: &mut [u32; 16] = Box::leak(Box::new([0u32; 16]));
        MOCK_TSEN_PTR.with(|p| p.set(mock.as_mut_ptr()));
        TsenDriver::new()
    }

    fn mock_ptr() -> *mut u32 {
        MOCK_TSEN_PTR.with(|p| p.get())
    }

    fn rd(offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(mock_ptr().add(offset / 4)) }
    }

    fn wr(offset: usize, val: u32) {
        unsafe { core::ptr::write_volatile(mock_ptr().add(offset / 4), val) }
    }

    #[test]
    fn test_tsen_enable_sets_en_bit() {
        let mut d = make_driver();
        d.enable();
        assert_ne!(rd(OFF_CONFIG) & 1, 0);
    }

    #[test]
    fn test_tsen_disable_clears_en_bit() {
        let mut d = make_driver();
        d.enable();
        d.disable();
        assert_eq!(rd(OFF_CONFIG) & 1, 0);
    }

    #[test]
    fn test_tsen_start_conversion_sets_bit1() {
        let mut d = make_driver();
        d.start_conversion();
        assert_ne!(rd(OFF_CONFIG) & (1 << 1), 0);
    }

    #[test]
    fn test_tsen_continuous_mode() {
        let mut d = make_driver();
        d.enable_continuous();
        assert_ne!(rd(OFF_CONFIG) & (1 << 2), 0);
        d.disable_continuous();
        assert_eq!(rd(OFF_CONFIG) & (1 << 2), 0);
    }

    #[test]
    fn test_tsen_is_busy() {
        let d = make_driver();
        wr(OFF_STATUS, 1);
        assert!(d.is_busy());
        wr(OFF_STATUS, 0);
        assert!(!d.is_busy());
    }

    #[test]
    fn test_tsen_is_done() {
        let d = make_driver();
        wr(OFF_STATUS, 1 << 1);
        assert!(d.is_done());
    }

    #[test]
    fn test_tsen_read_raw_masks_12bit() {
        let d = make_driver();
        wr(OFF_DATA, 0xFFFF_0ABC);
        assert_eq!(d.read_raw(), 0x0ABC);
    }

    // Pure conversion tests — no mock needed.

    #[test]
    fn test_raw_to_decicelsius_at_midpoint() {
        // raw = OFFSET (2048) → 0 decicelsius → 0.0 °C
        assert_eq!(TsenDriver::raw_to_decicelsius(TSEN_OFFSET as u16), 0);
    }

    #[test]
    fn test_raw_to_decicelsius_above_midpoint() {
        // raw = 2048 + 73 → 10 decicelsius → 1.0 °C
        let raw = (TSEN_OFFSET + TSEN_GAIN_X10) as u16;
        assert_eq!(TsenDriver::raw_to_decicelsius(raw), 10);
    }

    #[test]
    fn test_raw_to_decicelsius_below_midpoint() {
        // raw = 2048 - 73 → -10 decicelsius → -1.0 °C
        let raw = (TSEN_OFFSET - TSEN_GAIN_X10) as u16;
        assert_eq!(TsenDriver::raw_to_decicelsius(raw), -10);
    }

    #[test]
    fn test_tsen_read_decicelsius_integration() {
        let d = make_driver();
        // For exactly 250 decicelsius (25.0 °C):
        //   raw = OFFSET + 250 * GAIN_X10 / 10 = 2048 + 250 * 73 / 10 = 2048 + 1825 = 3873
        wr(OFF_DATA, 3873);
        let temp = d.read_decicelsius();
        assert_eq!(temp, 250, "25.0 °C should read as 250 decicelsius");
    }
}
