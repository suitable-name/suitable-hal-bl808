#![no_std]
#![no_main]

use bl808_hal::codec::{Codec, PCLK_ALL};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut codec = Codec::new(dp.codec);
    codec.enable();
    codec.force_clocks_on(PCLK_ALL);

    loop {
        core::hint::spin_loop();
    }
}
