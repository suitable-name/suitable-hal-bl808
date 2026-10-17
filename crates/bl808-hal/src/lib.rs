#![no_std]

//! Hardware Abstraction Layer (HAL) for the Bouffalo Lab BL808 SoC.
//!
//! This crate provides type-safe access to the BL808 peripherals.
//! It relies on the `bl808-pac` for raw register access.

pub use bl808_pac as pac;
pub use embedded_hal;

// --- Core & System ---
#[cfg(feature = "m0")]
pub mod aon;
pub mod debug;
#[cfg(feature = "m0")]
pub mod efuse;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod glb;
#[cfg(feature = "m0")]
pub mod hbn;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod ipc;
#[cfg(feature = "m0")]
pub mod sec;
pub mod misc;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod boards;

// --- Memory ---
#[cfg(feature = "m0")]
pub mod flash;
#[cfg(feature = "d0")]
pub mod psram;
#[cfg(feature = "m0")]
pub mod sdh;

// --- Connectivity ---
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod dma;
#[cfg(feature = "m0")]
pub mod emac;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod i2c;
#[cfg(feature = "m0")]
pub mod ir;
#[cfg(feature = "m0")]
pub mod iso11898;
#[cfg(any(feature = "m0", feature = "d0"))]
pub mod lz4d;
#[cfg(feature = "d0")]
pub mod mmglb;
#[cfg(any(feature = "m0", feature = "d0"))]
pub mod pds;
#[cfg(any(feature = "m0", feature = "d0"))]
pub mod pta;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod pwm;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod spi;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod timer;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod uart;
#[cfg(feature = "m0")]
pub mod usb;
#[cfg(feature = "m0")]
pub mod wifi;

// --- Analog ---
#[cfg(feature = "m0")]
pub mod adc;
#[cfg(feature = "m0")]
pub mod gpip;

// --- Audio ---
pub mod agc;
#[cfg(any(feature = "m0", feature = "d0"))]
pub mod audio;
#[cfg(feature = "d0")]
pub mod codec;
#[cfg(feature = "m0")]
pub mod i2s;
pub mod pdm;

// --- Video & AI ---
#[cfg(feature = "m0")]
pub mod cci;
pub mod common;
#[cfg(feature = "d0")]
pub mod dsi;
pub mod nec;
#[cfg(feature = "d0")]
pub mod dvp;
pub mod gpdac;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod gpio;
#[cfg(feature = "d0")]
pub mod h264;
#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
pub mod interrupts;
#[cfg(feature = "d0")]
pub mod isp;
#[cfg(feature = "d0")]
pub mod mipi;
#[cfg(feature = "d0")]
pub mod mjpeg;
#[cfg(feature = "d0")]
pub mod npu;
pub mod osd;
pub mod prelude;
pub mod tsen;
