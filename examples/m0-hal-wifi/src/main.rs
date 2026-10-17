#![no_std]
#![no_main]

use bl808_hal::wifi::WifiDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut wifi = WifiDriver::new(dp.wifi);
    wifi.enable_coex();
    let _state = wifi.get_coex_state();

    loop {
        core::hint::spin_loop();
    }
}
