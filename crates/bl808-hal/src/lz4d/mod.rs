//! Hardware LZ4 Decompressor (LZ4D) Driver
//!
//! The LZ4D engine decompresses LZ4-frame data autonomously via DMA.
//! The caller provides source (compressed) and destination (output) start
//! addresses; the engine reads until it reaches the LZ4 end-of-stream
//! marker and then signals completion via interrupt status.
//!
//! # Address encoding
//! The LZ4D register set splits every 32-bit address into a 6-bit *base*
//! (bits 31:26) and a 26-bit *offset* (bits 25:0).  The HAL encodes them
//! transparently: `base = addr >> 26`, `offset = addr & 0x03FF_FFFF`.

use bl808_pac::Lz4d as PacLz4d;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_LZ4D_PTR: core::cell::Cell<*const bl808_pac::lz4d::RegisterBlock>
        = const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lz4dError {
    /// Hardware reported a decompression error (corrupt/unsupported stream).
    DecompressError,
    /// Polling timed out before the engine signalled completion.
    Timeout,
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

pub struct Lz4d {
    _inner: PacLz4d,
}

impl Lz4d {
    pub fn new(lz4d: PacLz4d) -> Self {
        let this = Self { _inner: lz4d };
        // Pre-enable done and error interrupts.
        this.regs()
            .lz4_int_en()
            .write(|w| w.lz4_done_en().set_bit().lz4_err_en().set_bit());
        this
    }

    fn regs(&self) -> &bl808_pac::lz4d::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacLz4d::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_LZ4D_PTR.with(|p| p.get()) };
    }

    /// Split a 32-bit address into the LZ4D (base, offset) encoding.
    fn split_addr(addr: u32) -> (u8, u32) {
        ((addr >> 26) as u8, addr & 0x03FF_FFFF)
    }

    /// Configure the compressed-data source start address.
    pub fn set_source_address(&mut self, addr: u32) {
        let (base, offset) = Self::split_addr(addr);
        self.regs().lz4_src_start().write(|w| unsafe {
            w.lz4_src_start().bits(offset).lz4_src_base().bits(base)
        });
    }

    /// Configure the decompressed-data destination start address.
    pub fn set_dest_address(&mut self, addr: u32) {
        let (base, offset) = Self::split_addr(addr);
        self.regs().lz4_dst_start().write(|w| unsafe {
            w.lz4_dst_start().bits(offset).lz4_dst_base().bits(base)
        });
    }

    /// Enable the LZ4 engine and start decompression.
    pub fn trigger_start(&mut self) {
        self.regs()
            .lz4_config()
            .modify(|_, w| w.lz4_en().set_bit());
    }

    /// Disable the LZ4 engine.
    pub fn stop(&mut self) {
        self.regs()
            .lz4_config()
            .modify(|_, w| w.lz4_en().clear_bit());
    }

    /// Enable LZ4 checksum verification (frame checksum).
    pub fn set_checksum_enabled(&mut self, enabled: bool) {
        self.regs()
            .lz4_config()
            .modify(|_, w| w.lz4_checksum().bit(enabled));
    }

    /// Read the hardware control-state monitor (0 = idle).
    pub fn state(&self) -> u8 {
        self.regs().lz4_monitor().read().lz4_cs().bits()
    }

    /// Decompress LZ4-frame data from `src` to `dst`, blocking until done.
    ///
    /// Returns the number of bytes written to `dst` on success (read from the
    /// hardware `lz4_dst_end` status register).
    ///
    /// # Safety
    /// `src` must point to a valid LZ4 frame of at least `src_len` bytes.
    /// `dst` must be writable and large enough for the decompressed output.
    pub unsafe fn decompress(
        &mut self,
        src: *const u8,
        dst: *mut u8,
    ) -> Result<u32, Lz4dError> {
        self.set_source_address(src as u32);
        self.set_dest_address(dst as u32);

        // Disable first to reset state, then enable.
        self.stop();
        self.trigger_start();

        let mut timeout = 10_000_000_u32;
        loop {
            let sta = self.regs().lz4_int_sta().read();
            if sta.lz4_err_sta().bit_is_set() {
                self.stop();
                return Err(Lz4dError::DecompressError);
            }
            if sta.lz4_done_sta().bit_is_set() {
                self.stop();
                // dst_end holds the final write address (offset portion).
                let dst_end_off = self.regs().lz4_dst_end().read().lz4_dst_end().bits();
                let dst_start = dst as u32 & 0x03FF_FFFF;
                return Ok(dst_end_off.saturating_sub(dst_start));
            }
            timeout -= 1;
            if timeout == 0 {
                self.stop();
                return Err(Lz4dError::Timeout);
            }
            core::hint::spin_loop();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::lz4d::RegisterBlock;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 256] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 256]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_LZ4D_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_lz4d_source_address_encoding() {
        let regs = setup();
        let mut driver = Lz4d::new(unsafe { PacLz4d::steal() });

        driver.set_source_address(0x2300_0000);

        let reg = regs.lz4_src_start().read();
        assert_eq!(reg.lz4_src_base().bits(), (0x2300_0000u32 >> 26) as u8);
        assert_eq!(reg.lz4_src_start().bits(), 0x2300_0000 & 0x03FF_FFFF);
    }

    #[test]
    fn test_lz4d_dest_address_encoding() {
        let regs = setup();
        let mut driver = Lz4d::new(unsafe { PacLz4d::steal() });

        driver.set_dest_address(0x6200_0000);

        let reg = regs.lz4_dst_start().read();
        assert_eq!(reg.lz4_dst_base().bits(), (0x6200_0000u32 >> 26) as u8);
        assert_eq!(reg.lz4_dst_start().bits(), 0x6200_0000 & 0x03FF_FFFF);
    }

    #[test]
    fn test_lz4d_enable_bit() {
        let regs = setup();
        let mut driver = Lz4d::new(unsafe { PacLz4d::steal() });

        driver.trigger_start();
        assert!(regs.lz4_config().read().lz4_en().bit_is_set(), "lz4_en should be set");

        driver.stop();
        assert!(regs.lz4_config().read().lz4_en().bit_is_clear(), "lz4_en should be cleared");
    }
}
