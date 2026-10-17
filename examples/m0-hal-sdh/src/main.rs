#![no_std]
#![no_main]

use bl808_hal::sdh::SdhDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let _sdh = SdhDriver::new(dp.sdh);

    loop {
        core::hint::spin_loop();
    }
}
