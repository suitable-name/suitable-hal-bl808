#![no_std]
#![no_main]

use bl808_hal::i2s::I2sDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = I2sDriver::new(dp.i2s);
    let (mut tx, _rx) = driver.split();
    tx.enable();

    loop {
        core::hint::spin_loop();
    }
}
