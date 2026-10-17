//! blinky_d0 — BL808 C906 (D0) LED blink example for the Sipeed M1s Dock
//!
//! # Hardware
//!
//! Sipeed M1s Dock board:
//!
//! - LED1 (red) is on GPIO 8, active-low: anode to 3.3 V, cathode through
//!   1 kΩ to GPIO 8. Driving the pin LOW turns the LED on; HIGH turns it off.
//!
//! # Software
//!
//! 1. `Peripherals::take()` obtains the PAC singleton (requires the
//!    `critical-section` PAC feature, enabled transitively by the `d0` HAL
//!    feature).
//! 2. `dp.glb.split()` consumes the GLB peripheral and returns a
//!    `(GlbClockControl, Pins)` pair. The clock-control token is unused here
//!    because the boot-ROM has already configured a usable clock tree.
//! 3. `pins.P8.into_push_pull_output()` reconfigures GPIO 8 as a
//!    software-controlled push-pull output (func_sel = GPIO, mode = 1, OE set).
//! 4. The blink loop drives the pin low (LED on) then high (LED off) with a
//!    spin-delay between each transition.
//!
//! # SDK reference
//!
//! The M1s Dock schematic (Sipeed, rev 1.1) places LED1 on GPIO 8 with a
//! series resistor to 3.3 V — confirmed by the community-maintained pin-map
//! and the Sipeed SDK `hal_board.c` GPIO table for this board variant.

#![no_std]
#![no_main]
// On the host target (used by `cargo clippy` in the workspace) the `#[entry]`
// macro is not the real program entry, so Clippy sees helpers called from
// `main` as dead code.  This is expected for no_std firmware.
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    // Obtain exclusive ownership of all peripherals.
    // `take()` is available because the `d0` HAL feature enables the
    // `critical-section` PAC feature.
    let dp = bl808_hal::pac::Peripherals::take().unwrap();

    // Split the GLB peripheral into a clock-control token and the pin
    // collection.  The `GlbExt::split` impl (from `prelude::*`) marks GLB
    // as configured so that subsequent GPIO register writes go through the
    // correct base address.
    let (_clocks, pins) = dp.glb.split();

    // Configure GPIO 8 (M1s Dock LED1, red, active-low) as push-pull output.
    // `into_push_pull_output` sets: OE=1, IE=0, func_sel=GPIO, mode=1.
    let mut led = pins.P8.into_push_pull_output();

    loop {
        // LED on: drive GPIO 8 low (active-low LED).
        led.set_low().unwrap();
        spin_delay(1_000_000);

        // LED off: drive GPIO 8 high.
        led.set_high().unwrap();
        spin_delay(1_000_000);
    }
}

/// Busy-wait for approximately `cycles` spin-loop iterations.
///
/// At 400 MHz (D0 default after MM-PLL init) each `spin_loop` hint is
/// roughly 1–4 cycles, giving a wall-clock delay of ~1–4 ms per
/// 1 000 000 iterations. For a blink demo the exact timing does not matter.
fn spin_delay(cycles: u32) {
    for _ in 0..cycles {
        core::hint::spin_loop();
    }
}
