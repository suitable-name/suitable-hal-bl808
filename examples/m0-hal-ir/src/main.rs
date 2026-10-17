#![no_std]
#![no_main]

use bl808_hal::ir::IrDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = IrDriver::new(dp.ir);
    let (_tx, _rx) = driver.split();

    loop {
        core::hint::spin_loop();
    }
}
