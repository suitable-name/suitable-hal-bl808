#![no_std]
#![no_main]

use bl808_hal::sec::Sec;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut sec = Sec::new(dp.sec_eng);
    let key = [0x01020304u32, 0x05060708, 0x090A0B0C, 0x0D0E0F10];
    sec.load_aes_key(&key);
    sec.trigger_sha256();
    let _busy = sec.is_busy();

    loop {
        core::hint::spin_loop();
    }
}
