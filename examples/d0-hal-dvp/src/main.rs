#![no_std]
#![no_main]

use bl808_hal::dvp::{Config, Dvp, Polarity};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut dvp = Dvp::new(dp.dvp0);
    dvp.init(Config {
        vsync_polarity: Polarity::ActiveHigh,
        hsync_polarity: Polarity::ActiveHigh,
        pclk_polarity: Polarity::ActiveHigh,
        width: 640,
        height: 480,
    });

    loop {
        core::hint::spin_loop();
    }
}
