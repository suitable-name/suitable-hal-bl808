#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut i2c = I2c::new(dp.i2c2, &clocks, 400_000);
    let mut buf = [0u8; 1];
    let _ = i2c.read(0x50, &mut buf);

    loop {
        core::hint::spin_loop();
    }
}
