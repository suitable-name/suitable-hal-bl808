#![no_std]
#![no_main]

use bl808_hal::cci::Cci;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut cci = Cci::new(dp.cci);
    cci.enable();
    cci.set_clock_div(1);

    loop {
        core::hint::spin_loop();
    }
}
