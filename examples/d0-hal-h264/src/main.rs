#![no_std]
#![no_main]

use bl808_hal::h264::H264Encoder;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut enc = H264Encoder::new(dp.h264);
    enc.enable_clocks();

    loop {
        core::hint::spin_loop();
    }
}
