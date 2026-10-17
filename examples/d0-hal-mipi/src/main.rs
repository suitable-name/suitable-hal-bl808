#![no_std]
#![no_main]

use bl808_hal::mipi::MipiDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut mipi = MipiDriver::new();
    mipi.set_dual_lane(true);
    mipi.enable();

    loop {
        core::hint::spin_loop();
    }
}
