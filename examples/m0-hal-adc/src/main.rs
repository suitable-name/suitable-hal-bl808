#![no_std]
#![no_main]

use bl808_hal::adc::{Adc, AdcChannel};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut adc = Adc::new(dp.aon, dp.gpip);
    let _val = adc.read(AdcChannel::Ch0);

    loop {
        core::hint::spin_loop();
    }
}
