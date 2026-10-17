#![no_std]
#![no_main]

use bl808_hal::audio::{AudioDriver, ClockConfig};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut audio = AudioDriver::new(dp.audio);
    audio.set_clock(ClockConfig::Rate44_1k);

    loop {
        core::hint::spin_loop();
    }
}
