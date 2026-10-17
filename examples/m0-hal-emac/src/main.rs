#![no_std]
#![no_main]

use bl808_hal::emac::EthernetMac;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut bd_storage = [0u32; 64];
    let _emac = unsafe { EthernetMac::new(dp.emac, bd_storage.as_mut_ptr()) };

    loop {
        core::hint::spin_loop();
    }
}
