#![no_std]
#![no_main]

use bl808_hal::efuse::Efuse;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut efuse = Efuse::new(dp.efuse);
    efuse.trigger_read();
    let _wp = efuse.is_write_protected();

    loop {
        core::hint::spin_loop();
    }
}
