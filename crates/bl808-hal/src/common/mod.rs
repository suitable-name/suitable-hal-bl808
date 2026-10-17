//! Common HAL Types and Traits
//!
//! This module provides shared definitions used across multiple drivers in the HAL.
//! It includes a centralized error type for common hardware abstraction issues.

/// A common error type for HAL operations.
///
/// This enum represents errors that can occur at the HAL level, which are not
/// specific to a single peripheral's protocol errors (like `I2cError::Nack`).
/// It's used for issues like invalid configuration, peripheral state mismatches,
/// or clock setup failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    /// The provided configuration is invalid for the hardware.
    ///
    /// For example, requesting a baud rate that cannot be achieved with the given
    /// peripheral clock frequency.
    InvalidConfiguration,

    /// The peripheral is not in the correct state to perform the requested operation.
    ///
    /// For example, trying to write to a peripheral that has not been enabled yet.
    WrongState,

    /// The clock for the peripheral has not been enabled or is not configured correctly.
    ///
    /// Most peripherals on the BL808 require an explicit clock gate to be enabled in
    /// the GLB or MMGLB before they can be accessed.
    ClockNotEnabled,

    /// A timeout occurred while waiting for a hardware flag or state change.
    Timeout,
}

/// A specialized `Result` type for HAL operations.
pub type HalResult<T> = Result<T, HalError>;

/// Trait for peripherals that can be enabled or disabled.
pub trait Toggle {
    /// Enables the peripheral.
    fn enable(&mut self);

    /// Disables the peripheral.
    fn disable(&mut self);

    /// Returns `true` if the peripheral is currently enabled.
    fn is_enabled(&self) -> bool;
}

/// A simple, blocking delay provider.
///
/// This is a basic spin-loop delay for use in initialization sequences where a
/// precise timer may not yet be available. It is not accurate and should be
/// replaced with a proper timer-based delay for application logic.
///
/// # Warning
/// The duration of the delay is dependent on the CPU clock speed and compiler optimizations.
/// Use with caution.
pub fn busy_wait_ms(ms: u32) {
    // This is a rough approximation. A 480MHz CPU might execute around
    // 100,000 to 200,000 loop iterations per millisecond. We'll use a conservative
    // value. This must be calibrated for a specific clock speed and optimization level.
    let iterations_per_ms = 100_000;
    for _ in 0..(ms * iterations_per_ms) {
        // The `nop` instruction is a hint to the processor to do nothing.
        // It also prevents the compiler from optimizing the loop away entirely.
        riscv::asm::nop();
    }
}
