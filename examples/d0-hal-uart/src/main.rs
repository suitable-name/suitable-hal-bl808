#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use core::fmt::Write;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let tx = pins.P16.into_alternate::<7>();
    let rx = pins.P17.into_alternate::<7>();
    let mut serial = Serial::new(dp.uart3, tx, rx, 2000000, clocks.bclk);
    let _ = write!(serial, "D0 HAL UART3 initialized\r\n");

    loop {
        core::hint::spin_loop();
    }
}
