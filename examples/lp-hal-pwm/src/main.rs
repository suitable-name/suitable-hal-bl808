#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = PwmDriver::new(dp.pwm);
    let (mut grp0, _grp1) = driver.split();
    grp0.set_period(1000);
    grp0.set_duty(0, 500);
    grp0.enable();

    loop {
        core::hint::spin_loop();
    }
}
