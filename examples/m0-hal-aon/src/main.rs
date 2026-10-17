#![no_std]
#![no_main]

use bl808_hal::aon::Aon;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut aon = Aon::new(dp.aon);
    aon.enable_gpadc_continuous();

    loop {
        core::hint::spin_loop();
    }
}
