#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut timer = Timer::new(dp.timer0);
    timer.delay_ms(10);

    loop {
        core::hint::spin_loop();
    }
}
