#![no_std]
#![no_main]

use bl808_hal::pta::PtaDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut pta = PtaDriver::new(dp.pta);
    pta.set_config_0(0x00000001);

    loop {
        core::hint::spin_loop();
    }
}
