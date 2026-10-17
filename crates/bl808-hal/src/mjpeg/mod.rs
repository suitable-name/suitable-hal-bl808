//! Motion JPEG (Mjpeg) Encoder Driver

use bl808_pac::Mjpeg;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_MJPEG_PTR: core::cell::Cell<*const bl808_pac::mjpeg::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YuvFormat {
    Yuv422 = 0,
    Yuv420 = 1,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub width: u16,
    pub height: u16,
    pub format: YuvFormat,
}

pub struct MjpegEncoder {
    _mjpeg: Mjpeg,
}

impl MjpegEncoder {
    pub fn new(mjpeg: Mjpeg) -> Self {
        Self { _mjpeg: mjpeg }
    }

    fn regs(&self) -> &bl808_pac::mjpeg::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*Mjpeg::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_MJPEG_PTR.with(|p| p.get()) };
    }

    /// Enable the MJPEG encoder, program the frame size (in 8-pixel blocks) and
    /// YUV format, then configure YUV mode.
    ///
    /// `MJPEG_FRAME_SIZE` (offset 0x24) stores dimensions in **8-pixel blocks**,
    /// not pixels: `wblk = (width + 7) / 8`, `hblk = (height + 7) / 8`.
    pub fn init(&mut self, config: Config) {
        let regs = self.regs();
        regs.mjpeg_control_1().modify(|_, w| {
            w.reg_mjpeg_enable().set_bit()
        });

        // MJPEG_FRAME_SIZE (0x24): dimensions are in 8-pixel MCU blocks.
        let wblk = (config.width as u32).div_ceil(8);
        let hblk = (config.height as u32).div_ceil(8);
        regs.mjpeg_frame_size().write(|w| unsafe {
            w.reg_frame_wblk()
                .bits(wblk as u16)
                .reg_frame_hblk()
                .bits(hblk as u16)
        });

        regs.mjpeg_control_1().modify(|_, w| unsafe {
            w.reg_yuv_mode().bits(config.format as u8)
        });
    }

    /// Write the 64-entry JPEG quantisation table into the hardware Q-param
    /// registers (`MJPEG_Q_PARAM_[0..32]`, at 0x400–0x47C).
    ///
    /// Each `MJPEG_Q_PARAM_` register packs two consecutive 8-bit table entries:
    /// * bits [11:0]  → even-indexed entry (`reg_q_low`, zero-extended to 12 bits)
    /// * bits [27:16] → odd-indexed entry  (`reg_q_high`, zero-extended to 12 bits)
    ///
    /// The 64 bytes are stored in 32 registers (pairs 0/1, 2/3, …, 62/63).
    pub fn load_q_table(&mut self, table: &[u8; 64]) {
        let regs = self.regs();
        for i in 0..32 {
            let q_low  = table[i * 2]     as u16;
            let q_high = table[i * 2 + 1] as u16;
            regs.mjpeg_q_param_(i).write(|w| unsafe {
                w.reg_q_low().bits(q_low).reg_q_high().bits(q_high)
            });
        }
    }

    /// Trigger a software-initiated encode cycle.
    pub fn start(&mut self) {
        self.regs().mjpeg_control_2().modify(|_, w| w.reg_mjpeg_sw_run().set_bit());
    }

    /// Return the encoded size of frame 0 in bytes.
    ///
    /// Reads `frame_bit_cnt_0` from `MJPEG_BIT_CNT0` (offset 0x84) and
    /// converts bits to bytes by dividing by 8.
    pub fn get_encoded_size(&self) -> u32 {
        self.regs().mjpeg_bit_cnt0().read().frame_bit_cnt_0().bits() / 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::mjpeg::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        // MJPEG_Q_PARAM_[63] is at 0x400 + 31×4 = 0x47C; 1024 u32s (4 KiB) covers it.
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_MJPEG_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    /// `load_q_table` packs two 8-bit table entries into each `MJPEG_Q_PARAM_`
    /// register:  entry[2i] → reg_q_low [11:0], entry[2i+1] → reg_q_high [27:16].
    #[test]
    fn test_mjpeg_q_table_packing() {
        let regs = setup();
        let mjpeg = unsafe { Mjpeg::steal() };
        let mut enc = MjpegEncoder::new(mjpeg);

        // table[4] = 0xAA (pair index 2, q_low),  table[5] = 0xBB (pair index 2, q_high)
        // table[62] = 0x11 (pair index 31, q_low), table[63] = 0x22 (pair index 31, q_high)
        let mut table = [0u8; 64];
        table[4]  = 0xAA;
        table[5]  = 0xBB;
        table[62] = 0x11;
        table[63] = 0x22;

        enc.load_q_table(&table);

        // Pair 2: reg_q_low = 0xAA (bits[11:0]), reg_q_high = 0xBB (bits[27:16])
        let p2 = regs.mjpeg_q_param_(2).read();
        assert_eq!(p2.reg_q_low().bits(),  0xAA, "pair 2 low  (table[4]) should be 0xAA");
        assert_eq!(p2.reg_q_high().bits(), 0xBB, "pair 2 high (table[5]) should be 0xBB");

        // Pair 31 (last): reg_q_low = 0x11, reg_q_high = 0x22
        let p31 = regs.mjpeg_q_param_(31).read();
        assert_eq!(p31.reg_q_low().bits(),  0x11, "pair 31 low  (table[62]) should be 0x11");
        assert_eq!(p31.reg_q_high().bits(), 0x22, "pair 31 high (table[63]) should be 0x22");
    }

    /// `get_encoded_size` reads `MJPEG_BIT_CNT0` (offset 0x84) via PAC and divides by 8.
    #[test]
    fn test_mjpeg_bitstream_size_read() {
        let regs = setup();
        // Inject 800 bits into MJPEG_BIT_CNT0 (read-only field; write via raw ptr)
        unsafe {
            core::ptr::write_volatile(regs.mjpeg_bit_cnt0().as_ptr(), 800);
        }
        let mjpeg = unsafe { Mjpeg::steal() };
        let enc = MjpegEncoder::new(mjpeg);

        assert_eq!(enc.get_encoded_size(), 100, "800 bits / 8 = 100 bytes");
    }

    /// `start` must set `reg_mjpeg_sw_run` in MJPEG_CONTROL_2.
    #[test]
    fn test_mjpeg_start_trigger() {
        let regs = setup();
        let mjpeg = unsafe { Mjpeg::steal() };
        let mut enc = MjpegEncoder::new(mjpeg);

        enc.start();

        // reg_mjpeg_sw_run is bit 9 of MJPEG_CONTROL_2 (PAC-confirmed)
        let ctrl2_val = unsafe { core::ptr::read_volatile(regs.mjpeg_control_2().as_ptr()) };
        assert_ne!(ctrl2_val & (1 << 9), 0, "reg_mjpeg_sw_run (bit 9) should be set");
    }

    /// `init` must store frame dimensions in **8-pixel MCU blocks**, not pixels.
    #[test]
    fn test_mjpeg_init_frame_size_in_blocks() {
        let regs = setup();
        let mjpeg = unsafe { Mjpeg::steal() };
        let mut enc = MjpegEncoder::new(mjpeg);

        // 640×480: each dimension divides exactly into 8-pixel blocks
        enc.init(Config { width: 640, height: 480, format: YuvFormat::Yuv422 });

        let fs = regs.mjpeg_frame_size().read();
        assert_eq!(fs.reg_frame_wblk().bits(), 80, "640 px / 8 = 80 blocks");
        assert_eq!(fs.reg_frame_hblk().bits(), 60, "480 px / 8 = 60 blocks");
    }

    /// `init` rounds up non-multiple-of-8 dimensions.
    #[test]
    fn test_mjpeg_init_frame_size_rounds_up() {
        let regs = setup();
        let mjpeg = unsafe { Mjpeg::steal() };
        let mut enc = MjpegEncoder::new(mjpeg);

        // 641×481: (641+7)/8 = 81, (481+7)/8 = 61
        enc.init(Config { width: 641, height: 481, format: YuvFormat::Yuv420 });

        let fs = regs.mjpeg_frame_size().read();
        assert_eq!(fs.reg_frame_wblk().bits(), 81, "(641+7)/8 = 81 blocks");
        assert_eq!(fs.reg_frame_hblk().bits(), 61, "(481+7)/8 = 61 blocks");
    }
}
