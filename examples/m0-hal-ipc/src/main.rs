#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut ipc = Ipc::new(dp.ipc);
    ipc.unmask_m0(0x0001);
    ipc.trigger_d0(0x0001);
    let _pending = ipc.pending_m0();
    ipc.clear_m0(0x0001);

    loop {
        core::hint::spin_loop();
    }
}
