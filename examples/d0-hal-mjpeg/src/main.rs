#![no_std]
#![no_main]

use bl808_hal::mjpeg::{Config, MjpegEncoder, YuvFormat};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut mjpeg = MjpegEncoder::new(dp.mjpeg);
    mjpeg.init(Config {
        width: 640,
        height: 480,
        format: YuvFormat::Yuv422,
    });

    loop {
        core::hint::spin_loop();
    }
}
