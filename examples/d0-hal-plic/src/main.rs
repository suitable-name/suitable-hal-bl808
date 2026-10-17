#![no_std]
#![no_main]

use bl808_hal::interrupts::{Plic, PLIC_BASE};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let plic = unsafe { Plic::new(PLIC_BASE, 1) };
    unsafe {
        plic.set_priority(1, 1);
        plic.enable(1);
    }

    loop {
        core::hint::spin_loop();
    }
}
