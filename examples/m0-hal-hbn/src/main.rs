#![no_std]
#![no_main]

use bl808_hal::hbn::Hbn;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut hbn = Hbn::new(dp.hbn);
    hbn.set_root_clk_sel(1);
    hbn.enable_rtc();

    loop {
        core::hint::spin_loop();
    }
}
