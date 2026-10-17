#![no_std]
#![no_main]

use bl808_hal::psram::{PsramUhs, UhsMemSize, UhsPageSize, UhsTemp};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut psram = PsramUhs::new(dp.psram_uhs);
    psram.init(2000, UhsMemSize::Mb64, UhsPageSize::Kb2, UhsTemp::Normal);

    loop {
        core::hint::spin_loop();
    }
}
