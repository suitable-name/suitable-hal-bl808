//! SD Host Controller (SDH) Driver
//!
//! SDHCI-compatible driver for the BL808 SDH peripheral.
//! The SDH register file uses 16-bit (u16) registers.
//!
//! Key register offsets (u16 halfword units):
//!   0x08 - sd_arg_low      (argument bits 15:0)
//!   0x0A - sd_arg_high     (argument bits 31:16)
//!   0x0C - sd_transfer_mode
//!   0x0E - sd_cmd
//!   0x10..0x16 - sd_resp_0..3
//!   0x20 - sd_buffer_data_port_0  (lower 16 bits of data port)
//!   0x22 - sd_buffer_data_port_1  (upper 16 bits of data port)
//!   0x24 - sd_present_state_1     (buffer_wr_en/buffer_rd_en)
//!   0x30 - sd_normal_int_status   (xfer_complete, tx_rdy, rx_rdy)

use bl808_pac::Sdh;

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_SDH_PTR: core::cell::Cell<*const bl808_pac::sdh::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait SdhInstance: core::ops::Deref<Target = bl808_pac::sdh::RegisterBlock> {
    fn ptr() -> *const bl808_pac::sdh::RegisterBlock;
}

impl SdhInstance for Sdh {
    fn ptr() -> *const bl808_pac::sdh::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return Sdh::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_SDH_PTR.with(|p| p.get());
    }
}

/// Block size for SD sector transfers.
pub const BLOCK_SIZE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdhError {
    Timeout,
    Error,
}

pub struct SdhDriver<SDH> {
    _sdh: SDH,
}

impl<SDH: SdhInstance> SdhDriver<SDH> {
    pub fn new(sdh: SDH) -> Self {
        Self { _sdh: sdh }
    }

    fn regs(&self) -> &bl808_pac::sdh::RegisterBlock {
        unsafe { &*SDH::ptr() }
    }

    /// Send an SD command (argument split into two 16-bit registers).
    pub fn send_command(&mut self, cmd: u16, arg: u32) {
        let regs = self.regs();
        regs.sd_arg_low()
            .write(|w| unsafe { w.bits((arg & 0xFFFF) as u16) });
        regs.sd_arg_high()
            .write(|w| unsafe { w.bits((arg >> 16) as u16) });
        regs.sd_cmd().write(|w| unsafe { w.bits(cmd) });
    }

    /// Read the 4 × 16-bit response words.
    pub fn get_response(&self) -> [u16; 4] {
        let regs = self.regs();
        [
            regs.sd_resp_0().read().bits(),
            regs.sd_resp_1().read().bits(),
            regs.sd_resp_2().read().bits(),
            regs.sd_resp_3().read().bits(),
        ]
    }

    /// Poll `sd_normal_int_status` until the given bitmask is set or timeout.
    fn wait_status_bits(&self, mask: u16) -> Result<(), SdhError> {
        // In practice this would use a real timeout counter.
        for _ in 0..100_000u32 {
            let status = self.regs().sd_normal_int_status().read();
            if status.err_int().bit_is_set() {
                return Err(SdhError::Error);
            }
            let bits = status.bits();
            if (bits & mask) == mask {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(SdhError::Timeout)
    }

    /// Read a 512-byte block at `block_addr` into `buf`.
    ///
    /// Sends CMD17 (SDHCI_READ_SINGLE_BLOCK, response R1) and reads
    /// 256 × 16-bit words from the buffer data port.
    pub fn read_block(
        &mut self,
        block_addr: u32,
        buf: &mut [u8; BLOCK_SIZE],
    ) -> Result<(), SdhError> {
        // CMD17: single block read (cmd index 17, response R1, data present)
        // sd_cmd layout: bits[13:8] = cmd index, bit 0 = response type, bit 5 = data present
        // Simplified: pass 0x113A (CMD17 with data-present + R1 response flags)
        self.send_command(0x113A, block_addr);

        let regs = self.regs();

        // Wait for command complete (bit 0)
        self.wait_status_bits(1)?;
        // Clear it (write-1-to-clear)
        regs.sd_normal_int_status()
            .write(|w| w.cmd_complete().clear_bit_by_one());

        // Wait for buffer read enable (rx_rdy, bit 5)
        self.wait_status_bits(1 << 5)?;
        regs.sd_normal_int_status()
            .write(|w| w.rx_rdy().clear_bit_by_one());

        // Read 512 bytes = 256 × u16 via buffer_data_port_0 / port_1
        for i in 0..128usize {
            let lo = regs.sd_buffer_data_port_0().read().cpu_data0().bits() as u32;
            let hi = regs.sd_buffer_data_port_1().read().cpu_data1().bits() as u32;
            let word = lo | (hi << 16);
            buf[i * 4] = (word & 0xFF) as u8;
            buf[i * 4 + 1] = ((word >> 8) & 0xFF) as u8;
            buf[i * 4 + 2] = ((word >> 16) & 0xFF) as u8;
            buf[i * 4 + 3] = ((word >> 24) & 0xFF) as u8;
        }

        // Wait for transfer complete (bit 1)
        self.wait_status_bits(1 << 1)?;
        regs.sd_normal_int_status()
            .write(|w| w.xfer_complete().clear_bit_by_one());

        Ok(())
    }

    /// Write a 512-byte block at `block_addr` from `buf`.
    ///
    /// Sends CMD24 (SDHCI_WRITE_BLOCK, response R1) and writes
    /// 256 × 16-bit words to the buffer data port.
    pub fn write_block(
        &mut self,
        block_addr: u32,
        buf: &[u8; BLOCK_SIZE],
    ) -> Result<(), SdhError> {
        // CMD24: single block write
        self.send_command(0x183A, block_addr);

        let regs = self.regs();

        // Wait for command complete
        self.wait_status_bits(1)?;
        regs.sd_normal_int_status()
            .write(|w| w.cmd_complete().clear_bit_by_one());

        // Wait for buffer write enable (tx_rdy, bit 4)
        self.wait_status_bits(1 << 4)?;
        regs.sd_normal_int_status()
            .write(|w| w.tx_rdy().clear_bit_by_one());

        // Write 512 bytes = 256 × u16 via buffer_data_port_0 / port_1
        for i in 0..128usize {
            let lo = u16::from_le_bytes([buf[i * 4], buf[i * 4 + 1]]);
            let hi = u16::from_le_bytes([buf[i * 4 + 2], buf[i * 4 + 3]]);
            regs.sd_buffer_data_port_0()
                .write(|w| unsafe { w.cpu_data0().bits(lo) });
            regs.sd_buffer_data_port_1()
                .write(|w| unsafe { w.cpu_data1().bits(hi) });
        }

        // Wait for transfer complete
        self.wait_status_bits(1 << 1)?;
        regs.sd_normal_int_status()
            .write(|w| w.xfer_complete().clear_bit_by_one());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::sdh::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u16; 1024] = Box::leak(Box::new([0u16; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_SDH_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_sdh_command_issue() {
        let regs = setup();
        let mut driver = SdhDriver::new(unsafe { Sdh::steal() });

        driver.send_command(0x1234, 0xABCD_EFFF);

        unsafe {
            assert_eq!(core::ptr::read_volatile(regs.sd_arg_low().as_ptr()), 0xEFFF);
            assert_eq!(core::ptr::read_volatile(regs.sd_arg_high().as_ptr()), 0xABCD);
            assert_eq!(core::ptr::read_volatile(regs.sd_cmd().as_ptr()), 0x1234);
        }
    }

    #[test]
    fn test_sdh_get_response() {
        let regs = setup();
        unsafe {
            core::ptr::write_volatile(regs.sd_resp_0().as_ptr(), 0x1111);
            core::ptr::write_volatile(regs.sd_resp_1().as_ptr(), 0x2222);
            core::ptr::write_volatile(regs.sd_resp_2().as_ptr(), 0x3333);
            core::ptr::write_volatile(regs.sd_resp_3().as_ptr(), 0x4444);
        }

        let driver = SdhDriver::new(unsafe { Sdh::steal() });

        let resp = driver.get_response();
        assert_eq!(resp, [0x1111, 0x2222, 0x3333, 0x4444]);
    }

    #[test]
    fn test_sdh_read_block_timeout_without_status() {
        let _regs = setup();
        // normal_int_status stays 0 → timeout
        let mut driver = SdhDriver::new(unsafe { Sdh::steal() });

        let mut buf = [0u8; 512];
        // CMD_COMPLETE bit never set → Timeout
        assert!(matches!(driver.read_block(0, &mut buf), Err(SdhError::Timeout)));
    }
}
