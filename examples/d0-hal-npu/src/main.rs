#![no_std]
#![no_main]

use bl808_hal::npu::Npu;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut npu = Npu::new(dp.npu);
    npu.stop();

    loop {
        core::hint::spin_loop();
    }
}
