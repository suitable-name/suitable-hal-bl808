#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use bl808_hal::dma::{BurstSize, TransferWidth};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let dma = Dma::new(dp.dma0);
    let mut channels = dma.split();
    let mut ch0 = channels.ch0.take().unwrap();

    let src = [0x11223344u32, 0x55667788u32];
    let mut dst = [0u32; 2];
    unsafe {
        let _ = ch0.transfer_mem2mem(&src, &mut dst, TransferWidth::Bits32, BurstSize::Burst1);
    }
    let _ = ch0.wait_transfer();

    loop {
        core::hint::spin_loop();
    }
}
