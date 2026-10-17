#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut pds = Pds::new(dp.pds);
    pds.set_wakeup_sources(WakeupSources::GPIO);

    loop {
        core::hint::spin_loop();
    }
}
