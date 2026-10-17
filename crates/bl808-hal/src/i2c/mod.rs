//! I2C Master Driver for BL808
//!
//! Implements [`embedded_hal::i2c::I2c`] for I2C0 (M0/D0), I2C1 (M0),
//! I2C2/I2C3 (D0).  Blocking only; transfers spin on FIFO and interrupt flags.
//!
//! # Hardware notes
//! - TX/RX FIFOs are 2 bytes deep each.
//! - Sub-address (register-address) support handles write-then-read with a
//!   hardware-generated REPEATED START for 1–4 byte register addresses.
//! - All period registers default to 0x0f0f_0f0f (15 bclk cycles per phase)
//!   which gives ~667 kHz at 40 MHz bclk; adjust via [`I2c::set_timing`] for
//!   100 kHz or 400 kHz operation.

use crate::glb::Clocks;
use bl808_pac::I2c0;
use embedded_hal::i2c::{self, ErrorKind, ErrorType, I2c as I2cTrait, NoAcknowledgeSource, Operation};

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_I2C0_PTR: core::cell::Cell<*const bl808_pac::i2c0::RegisterBlock>
        = const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cError {
    /// Device did not acknowledge its address.
    Nack,
    /// Bus arbitration was lost to another master.
    ArbitrationLost,
    /// FIFO overrun or underrun error.
    FifoError,
    /// Operation timed out (spin budget exhausted).
    Timeout,
}

impl i2c::Error for I2cError {
    fn kind(&self) -> ErrorKind {
        match self {
            I2cError::Nack => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
            I2cError::ArbitrationLost => ErrorKind::ArbitrationLoss,
            I2cError::FifoError => ErrorKind::Other,
            I2cError::Timeout => ErrorKind::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Instance trait
// ---------------------------------------------------------------------------

pub trait I2cInstance: core::ops::Deref<Target = bl808_pac::i2c0::RegisterBlock> {
    fn ptr() -> *const bl808_pac::i2c0::RegisterBlock;
}

impl I2cInstance for I2c0 {
    fn ptr() -> *const bl808_pac::i2c0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return I2c0::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_I2C0_PTR.with(|p| p.get());
    }
}

#[cfg(any(feature = "m0", feature = "lp"))]
impl I2cInstance for bl808_pac::I2c1 {
    fn ptr() -> *const bl808_pac::i2c0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return bl808_pac::I2c1::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_I2C0_PTR.with(|p| p.get());
    }
}

#[cfg(feature = "d0")]
impl I2cInstance for bl808_pac::I2c2 {
    fn ptr() -> *const bl808_pac::i2c0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return bl808_pac::I2c2::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_I2C0_PTR.with(|p| p.get());
    }
}

#[cfg(feature = "d0")]
impl I2cInstance for bl808_pac::I2c3 {
    fn ptr() -> *const bl808_pac::i2c0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return bl808_pac::I2c3::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_I2C0_PTR.with(|p| p.get());
    }
}

// ---------------------------------------------------------------------------
// Driver struct
// ---------------------------------------------------------------------------

/// Maximum number of spin iterations before a busy-wait returns `Err(Timeout)`.
const SPIN_BUDGET: u32 = 1_000_000;

/// I2C master driver.
pub struct I2c<I2C: I2cInstance> {
    _inner: I2C,
}

impl<I2C: I2cInstance> I2c<I2C> {
    /// Construct the driver, configuring timing for the given bus frequency.
    ///
    /// `bclk_hz` is the peripheral bus clock (from [`Clocks::bclk`]).
    /// `i2c_hz` is the desired SCL frequency (e.g. 100_000 or 400_000).
    pub fn new(i2c: I2C, clocks: &Clocks, i2c_hz: u32) -> Self {
        let driver = Self { _inner: i2c };
        driver.configure_timing(clocks.bclk, i2c_hz);
        driver
    }

    fn regs(&self) -> &bl808_pac::i2c0::RegisterBlock {
        unsafe { &*I2C::ptr() }
    }

    /// Compute and write period registers for the target SCL frequency.
    ///
    /// Each of the four 8-bit phase fields in the timing registers represents
    /// the number of bclk cycles for that SCL phase minus one.  A full SCL
    /// period has 4 such phases (total = 4 × (phase + 1) bclk cycles).
    fn configure_timing(&self, bclk_hz: u32, i2c_hz: u32) {
        let phase = (bclk_hz / (4 * i2c_hz)).saturating_sub(1).min(0xff) as u8;
        let val: u32 = (phase as u32)
            | ((phase as u32) << 8)
            | ((phase as u32) << 16)
            | ((phase as u32) << 24);
        let regs = self.regs();
        regs.i2c_prd_start().write(|w| unsafe { w.bits(val) });
        regs.i2c_prd_stop().write(|w| unsafe { w.bits(val) });
        regs.i2c_prd_data().write(|w| unsafe { w.bits(val) });
    }

    /// Wait until the I2C bus is not busy (previous transfer complete).
    ///
    /// Returns `Err(I2cError::Timeout)` if the bus stays busy for more than
    /// `SPIN_BUDGET` iterations.
    fn wait_bus_idle(&self) -> Result<(), I2cError> {
        let regs = self.regs();
        let mut budget = SPIN_BUDGET;
        while regs.i2c_bus_busy().read().sts_i2c_bus_busy().bit_is_set() {
            if budget == 0 {
                return Err(I2cError::Timeout);
            }
            budget -= 1;
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Clear TX and RX FIFOs.
    fn clear_fifos(&self) {
        self.regs().i2c_fifo_config_0().modify(|_, w| {
            w.tx_fifo_clr().clear_bit_by_one().rx_fifo_clr().clear_bit_by_one()
        });
    }

    /// Clear all interrupt flags.
    fn clear_interrupts(&self) {
        self.regs().i2c_int_sts().write(|w| {
            w.cr_i2c_end_clr()
                .clear_bit_by_one()
                .cr_i2c_nak_clr()
                .clear_bit_by_one()
                .cr_i2c_arb_clr()
                .clear_bit_by_one()
        });
    }

    /// Poll for transfer completion. Returns `Err(I2cError)` on NAK,
    /// arbitration loss, or timeout; `Ok(())` on success.
    fn wait_done(&self) -> Result<(), I2cError> {
        let regs = self.regs();
        let mut budget = SPIN_BUDGET;
        loop {
            let ints = regs.i2c_int_sts().read();
            if ints.i2c_nak_int().bit_is_set() {
                self.clear_interrupts();
                // Disable master to abort
                regs.i2c_config()
                    .modify(|_, w| w.cr_i2c_m_en().clear_bit());
                return Err(I2cError::Nack);
            }
            if ints.i2c_arb_int().bit_is_set() {
                self.clear_interrupts();
                return Err(I2cError::ArbitrationLost);
            }
            if ints.i2c_end_int().bit_is_set() {
                self.clear_interrupts();
                return Ok(());
            }
            if budget == 0 {
                return Err(I2cError::Timeout);
            }
            budget -= 1;
            core::hint::spin_loop();
        }
    }

    /// Write bytes without sub-address (straight write).
    fn do_write(&mut self, address: u8, bytes: &[u8]) -> Result<(), I2cError> {
        if bytes.is_empty() {
            // Probe: send address + STOP with zero data bytes.
            // Set pkt_len = 0 (hardware sends 0 data bytes after address).
            return self.do_write_zero_len(address);
        }

        let regs = self.regs();
        self.wait_bus_idle()?;
        self.clear_fifos();
        self.clear_interrupts();

        // Configure: slave address, packet length (len-1), write direction, no sub-addr.
        regs.i2c_config().modify(|_, w| unsafe {
            w.cr_i2c_slv_addr()
                .bits(address as u16)
                .cr_i2c_pkt_len()
                .bits((bytes.len() - 1) as u8)
                .cr_i2c_pkt_dir()
                .clear_bit()       // 0 = write
                .cr_i2c_sub_addr_en()
                .clear_bit()
                .cr_i2c_scl_sync_en()
                .set_bit()
        });

        // Pre-fill FIFO (max 2 bytes).
        let mut iter = bytes.iter();
        for byte in iter.by_ref().take(2) {
            regs.i2c_fifo_wdata()
                .write(|w| unsafe { w.bits(*byte as u32) });
        }

        // Trigger transfer.
        regs.i2c_config()
            .modify(|_, w| w.cr_i2c_m_en().set_bit());

        // Feed remaining bytes as FIFO drains.
        for byte in iter {
            let mut budget = SPIN_BUDGET;
            loop {
                let cnt = regs.i2c_fifo_config_1().read().tx_fifo_cnt().bits();
                if cnt < 2 {
                    break;
                }
                if budget == 0 {
                    return Err(I2cError::Timeout);
                }
                budget -= 1;
                core::hint::spin_loop();
            }
            regs.i2c_fifo_wdata()
                .write(|w| unsafe { w.bits(*byte as u32) });
        }

        self.wait_done()
    }

    /// Address-only write (probe / bus scan).
    fn do_write_zero_len(&mut self, address: u8) -> Result<(), I2cError> {
        let regs = self.regs();
        self.wait_bus_idle()?;
        self.clear_fifos();
        self.clear_interrupts();

        // Send a 1-byte dummy write (the hardware still sends START + ADDR).
        // Write a 0x00 to satisfy the hardware (pkt_len = 0 → 1 byte).
        regs.i2c_config().modify(|_, w| unsafe {
            w.cr_i2c_slv_addr()
                .bits(address as u16)
                .cr_i2c_pkt_len()
                .bits(0)
                .cr_i2c_pkt_dir()
                .clear_bit()
                .cr_i2c_sub_addr_en()
                .clear_bit()
                .cr_i2c_scl_sync_en()
                .set_bit()
        });
        regs.i2c_fifo_wdata()
            .write(|w| unsafe { w.bits(0) });
        regs.i2c_config()
            .modify(|_, w| w.cr_i2c_m_en().set_bit());
        self.wait_done()
    }

    /// Read bytes into `buffer`.
    fn do_read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), I2cError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let regs = self.regs();
        self.wait_bus_idle()?;
        self.clear_fifos();
        self.clear_interrupts();

        regs.i2c_config().modify(|_, w| unsafe {
            w.cr_i2c_slv_addr()
                .bits(address as u16)
                .cr_i2c_pkt_len()
                .bits((buffer.len() - 1) as u8)
                .cr_i2c_pkt_dir()
                .set_bit()        // 1 = read
                .cr_i2c_sub_addr_en()
                .clear_bit()
                .cr_i2c_scl_sync_en()
                .set_bit()
        });

        regs.i2c_config()
            .modify(|_, w| w.cr_i2c_m_en().set_bit());

        for slot in buffer.iter_mut() {
            let mut budget = SPIN_BUDGET;
            loop {
                let cnt = regs.i2c_fifo_config_1().read().rx_fifo_cnt().bits();
                if cnt > 0 {
                    break;
                }
                if budget == 0 {
                    return Err(I2cError::Timeout);
                }
                budget -= 1;
                core::hint::spin_loop();
            }
            *slot = regs.i2c_fifo_rdata().read().bits() as u8;
        }

        self.wait_done()
    }

    /// Write `bytes` then read `buffer` using hardware sub-address support
    /// (generates a REPEATED START).
    ///
    /// Supports up to 4 bytes of register address; longer write sequences
    /// fall back to two separate transactions (STOP + START between them).
    fn do_write_read(
        &mut self,
        address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), I2cError> {
        if buffer.is_empty() {
            return if bytes.is_empty() {
                Ok(())
            } else {
                self.do_write(address, bytes)
            };
        }

        if bytes.is_empty() {
            return self.do_read(address, buffer);
        }

        if bytes.len() <= 4 {
            // Use hardware sub-address (REPEATED START).
            self.do_write_read_subaddr(address, bytes, buffer)
        } else {
            // Fall back: STOP + START between write and read.
            self.do_write(address, bytes)?;
            self.do_read(address, buffer)
        }
    }

    fn do_write_read_subaddr(
        &mut self,
        address: u8,
        sub: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), I2cError> {
        let regs = self.regs();
        self.wait_bus_idle()?;
        self.clear_fifos();
        self.clear_interrupts();

        // Load sub-address bytes (little-endian order: b0 sent first).
        regs.i2c_sub_addr().modify(|_, w| unsafe {
            let b0 = *sub.first().unwrap_or(&0) as u32;
            let b1 = *sub.get(1).unwrap_or(&0) as u32;
            let b2 = *sub.get(2).unwrap_or(&0) as u32;
            let b3 = *sub.get(3).unwrap_or(&0) as u32;
            w.cr_i2c_sub_addr_b0()
                .bits(b0 as u8)
                .cr_i2c_sub_addr_b1()
                .bits(b1 as u8)
                .cr_i2c_sub_addr_b2()
                .bits(b2 as u8)
                .cr_i2c_sub_addr_b3()
                .bits(b3 as u8)
        });

        regs.i2c_config().modify(|_, w| unsafe {
            w.cr_i2c_slv_addr()
                .bits(address as u16)
                .cr_i2c_pkt_len()
                .bits((buffer.len() - 1) as u8)
                .cr_i2c_pkt_dir()
                .set_bit()
                .cr_i2c_sub_addr_en()
                .set_bit()
                .cr_i2c_sub_addr_bc()
                .bits((sub.len() - 1) as u8)
                .cr_i2c_scl_sync_en()
                .set_bit()
        });

        regs.i2c_config()
            .modify(|_, w| w.cr_i2c_m_en().set_bit());

        for slot in buffer.iter_mut() {
            let mut budget = SPIN_BUDGET;
            loop {
                if regs.i2c_fifo_config_1().read().rx_fifo_cnt().bits() > 0 {
                    break;
                }
                if budget == 0 {
                    return Err(I2cError::Timeout);
                }
                budget -= 1;
                core::hint::spin_loop();
            }
            *slot = regs.i2c_fifo_rdata().read().bits() as u8;
        }

        self.wait_done()
    }
}

// ---------------------------------------------------------------------------
// embedded-hal traits
// ---------------------------------------------------------------------------

impl<I2C: I2cInstance> ErrorType for I2c<I2C> {
    type Error = I2cError;
}

impl<I2C: I2cInstance> I2cTrait for I2c<I2C> {
    fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.do_read(address, buffer)
    }

    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.do_write(address, bytes)
    }

    fn write_read(
        &mut self,
        address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.do_write_read(address, bytes, buffer)
    }

    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        for op in operations.iter_mut() {
            match op {
                Operation::Read(buf) => self.do_read(address, buf)?,
                Operation::Write(bytes) => self.do_write(address, bytes)?,
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::i2c0::RegisterBlock;
    use crate::glb::Clocks;

    fn make_clocks() -> Clocks {
        Clocks { sys_clk: 40_000_000, hclk: 40_000_000, bclk: 40_000_000 }
    }

    #[test]
    fn test_i2c_configure_timing() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_I2C0_PTR.with(|p| p.set(mock_ptr));

        let i2c0 = unsafe { I2c0::steal() };
        let _i2c = I2c::new(i2c0, &make_clocks(), 100_000);

        // Period registers should have been written (non-zero).
        let prd_data_val = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x10 / 4))
        };
        assert_ne!(prd_data_val, 0, "Timing registers should be set on init");
    }

    #[test]
    fn test_i2c_config_write_direction() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_I2C0_PTR.with(|p| p.set(mock_ptr));

        let i2c0 = unsafe { I2c0::steal() };
        let mut i2c = I2c::new(i2c0, &make_clocks(), 100_000);

        // Write a byte — we can't actually do the transfer in mock, but verify
        // config register is set up with write direction (bit 1 = 0).
        let _ = i2c.write(0x42, &[0xAB]);

        let cfg = unsafe {
            core::ptr::read_volatile(mock_ptr as *const u32)
        };
        // cr_i2c_slv_addr is at bits 8:17, address 0x42 → bits 8:14 = 0x42
        assert_eq!((cfg >> 8) & 0xFF, 0x42, "Slave address should be 0x42");
        // cr_i2c_pkt_dir is bit 1 — should be 0 for write
        assert_eq!((cfg >> 1) & 1, 0, "Write direction should be 0");
    }

    #[test]
    fn test_i2c_config_read_direction() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_I2C0_PTR.with(|p| p.set(mock_ptr));

        let i2c0 = unsafe { I2c0::steal() };
        let mut i2c = I2c::new(i2c0, &make_clocks(), 100_000);

        // Mock: pre-fill rx_fifo_cnt so read doesn't hang.
        // fifo_config_1 is at offset 0x84; rx_fifo_cnt is bits 8:9 = 1.
        unsafe {
            core::ptr::write_volatile(
                (mock_ptr as *mut u32).add(0x84 / 4),
                1 << 8, // rx_fifo_cnt = 1
            );
        }

        let mut buf = [0u8; 1];
        let _ = i2c.read(0x55, &mut buf);

        let cfg = unsafe {
            core::ptr::read_volatile(mock_ptr as *const u32)
        };
        assert_eq!((cfg >> 8) & 0xFF, 0x55, "Slave address should be 0x55");
        // cr_i2c_pkt_dir is bit 1 — should be 1 for read
        assert_eq!((cfg >> 1) & 1, 1, "Read direction should be 1");
    }
}
