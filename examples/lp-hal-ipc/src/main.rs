#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut ipc = Ipc::new(dp.ipc);
    ipc.trigger_m0(0x0004);
    ipc.trigger_d0(0x0004);

    loop {
        core::hint::spin_loop();
    }
}
