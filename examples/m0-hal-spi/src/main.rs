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
    let mut spi = Spi::new(dp.spi0, &clocks, 1_000_000, Polarity::IdleLow, Phase::CaptureOnFirstTransition, BitOrder::MsbFirst);
    let tx_data = [0x9Fu8];
    let mut rx_data = [0u8; 3];
    let _ = spi.transfer(&mut rx_data, &tx_data);

    loop {
        core::hint::spin_loop();
    }
}
