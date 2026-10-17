//! HAL Prelude
//!
//! `use bl808_hal::prelude::*;` to bring all extension traits, common types,
//! and embedded-hal re-exports into scope.

// GLB / clock
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::glb::{Clocks, GlbExt, GlbManager};

// GPIO
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::gpio::{
    Alternate, Analog, Floating, Input, OpenDrain, Output, Pin, PullDown, PullUp, PushPull,
};

// UART
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::uart::Serial;

// I2C
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::i2c::{I2c, I2cError, I2cInstance};

// SPI
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::spi::{BitOrder, Phase, Polarity, Spi, SpiError, SpiInstance};

// Timer / Watchdog
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::timer::{Timer, Watchdog};

// DMA
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::dma::{Channel, Channels, Dma};

// IPC
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::ipc::{ChannelMask, Ipc};

// PWM
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::pwm::{PwmDriver, PwmGroup};

// ADC
#[cfg(feature = "m0")]
pub use crate::adc::Adc;

// PDS
#[cfg(any(feature = "m0", feature = "d0"))]
pub use crate::pds::{GpioSet, GpioWakeupMode, Pds, WakeupSources};

// Interrupt controller (PLIC)
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use crate::interrupts::Plic;

// embedded-hal digital traits
pub use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};

// embedded-hal Delay traits
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use embedded_hal::delay::DelayNs;

// embedded-hal SPI traits
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use embedded_hal::spi::SpiBus;

// embedded-hal I2C traits
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use embedded_hal::i2c::I2c as I2cBus;

// embedded-hal-nb UART traits
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub use embedded_hal_nb::serial::{Read as SerialRead, Write as SerialWrite};

// core::fmt::Write
pub use core::fmt::Write as _;

