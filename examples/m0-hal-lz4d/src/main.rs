#![no_std]
#![no_main]

use bl808_hal::lz4d::Lz4d;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let _lz4d = Lz4d::new(dp.lz4d);

    loop {
        core::hint::spin_loop();
    }
}
