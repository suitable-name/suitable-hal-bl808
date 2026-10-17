#![no_std]
#![no_main]

use bl808_hal::tsen::TsenDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut tsen = TsenDriver::new();
    tsen.enable();
    tsen.start_conversion();
    let _raw = tsen.read_raw();
    let _temp = tsen.read_decicelsius();

    loop {
        core::hint::spin_loop();
    }
}
