#![no_std]
#![no_main]

use bl808_hal::mmglb::MmGlb;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut mmglb = MmGlb::new(dp.mm_glb);
    mmglb.enable_codec_clock();

    loop {
        core::hint::spin_loop();
    }
}
