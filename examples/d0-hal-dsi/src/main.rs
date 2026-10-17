#![no_std]
#![no_main]

use bl808_hal::dsi::{DsiDriver, DsiLanes};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut dsi = DsiDriver::new();
    dsi.set_lane_count(DsiLanes::Two);
    dsi.enable();

    loop {
        core::hint::spin_loop();
    }
}
