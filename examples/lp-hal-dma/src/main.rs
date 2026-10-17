#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let dma = Dma::new(dp.dma0);
    let _channels = dma.split();

    loop {
        core::hint::spin_loop();
    }
}
