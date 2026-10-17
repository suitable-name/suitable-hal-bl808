#![no_std]
#![no_main]

use bl808_hal::flash::Flash;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut flash = Flash::new(dp.sf_ctrl);
    let _id = flash.read_jedec_id();

    loop {
        core::hint::spin_loop();
    }
}
