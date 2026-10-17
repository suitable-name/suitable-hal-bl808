#![no_std]
#![no_main]

use bl808_hal::isp::IspDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut isp = IspDriver::new();
    isp.enable_pipeline();

    loop {
        core::hint::spin_loop();
    }
}
