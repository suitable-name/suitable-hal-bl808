#![no_std]
#![no_main]

use bl808_hal::usb::UsbDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut usb = UsbDriver::new(dp.usb);
    usb.enable_device_mode(true);

    loop {
        core::hint::spin_loop();
    }
}
