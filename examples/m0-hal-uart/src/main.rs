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
    let tx = pins.P14.into_alternate::<7>();
    let rx = pins.P15.into_alternate::<7>();
    let mut serial = Serial::new(dp.uart0, tx, rx, 2000000, clocks.bclk);
    let _ = write!(serial, "HAL UART M0 initialized @ 2MBaud\r\n");
    serial.write_bytes(b"Ready\r\n");

    loop {
        core::hint::spin_loop();
    }
}
