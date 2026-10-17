#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (_clk, pins) = dp.glb.split();
    let mut led = pins.P8.into_push_pull_output();
    let _ = led.set_low();
    let _ = led.set_high();

    loop {
        core::hint::spin_loop();
    }
}
