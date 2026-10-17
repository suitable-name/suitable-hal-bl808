#![no_std]
#![no_main]

use bl808_hal::iso11898::CanDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut can = CanDriver::new(dp.iso11898);
    can.set_reset_mode(true);
    can.set_baud_rate(4, 1, 5, 2, false);
    can.set_reset_mode(false);

    loop {
        core::hint::spin_loop();
    }
}
