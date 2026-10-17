//! SPI Master Driver for BL808
//!
//! Implements [`embedded_hal::spi::SpiBus`] for SPI0 (M0) and SPI1 (D0).
//! Transfers are blocking (FIFO-polled).
//!
//! # Hardware notes
//! - TX/RX FIFOs are 32 entries (bytes) deep.
//! - Full-duplex: each byte clocked out simultaneously shifts a byte in.
//! - Clock divider: SCL period = (prd_d_ph_0 + prd_d_ph_1 + 2) × bclk cycles.
//! - Frame size is fixed at 8-bit per word for the [`SpiBus<u8>`] implementation.

use crate::glb::Clocks;
use bl808_pac::Spi0;
use embedded_hal::spi::{self, ErrorKind, ErrorType, SpiBus};

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_SPI0_PTR: core::cell::Cell<*const bl808_pac::spi0::RegisterBlock>
        = const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    /// SPI bus overrun (RX FIFO read when empty, or TX FIFO write when full).
    Overrun,
    /// Operation timed out (spin budget exhausted).
    Timeout,
}

impl spi::Error for SpiError {
    fn kind(&self) -> ErrorKind {
        match self {
            SpiError::Overrun => ErrorKind::Overrun,
            SpiError::Timeout => ErrorKind::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Instance trait
// ---------------------------------------------------------------------------

pub trait SpiInstance: core::ops::Deref<Target = bl808_pac::spi0::RegisterBlock> {
    fn ptr() -> *const bl808_pac::spi0::RegisterBlock;
}

impl SpiInstance for Spi0 {
    fn ptr() -> *const bl808_pac::spi0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return Spi0::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_SPI0_PTR.with(|p| p.get());
    }
}

#[cfg(feature = "d0")]
impl SpiInstance for bl808_pac::Spi1 {
    fn ptr() -> *const bl808_pac::spi0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return bl808_pac::Spi1::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_SPI0_PTR.with(|p| p.get());
    }
}

// ---------------------------------------------------------------------------
// SPI mode
// ---------------------------------------------------------------------------

/// SPI clock polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    /// Clock idle low (CPOL = 0).
    IdleLow,
    /// Clock idle high (CPOL = 1).
    IdleHigh,
}

/// SPI clock phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Sample on first (leading) edge (CPHA = 0).
    CaptureOnFirstTransition,
    /// Sample on second (trailing) edge (CPHA = 1).
    CaptureOnSecondTransition,
}

/// SPI bit order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitOrder {
    /// Most significant bit first (standard).
    MsbFirst,
    /// Least significant bit first.
    LsbFirst,
}

// ---------------------------------------------------------------------------
// Driver struct
// ---------------------------------------------------------------------------

/// Maximum number of spin iterations before a busy-wait returns `Err(Timeout)`.
const SPIN_BUDGET: u32 = 1_000_000;

/// SPI master driver.
pub struct Spi<SPI: SpiInstance> {
    _inner: SPI,
}

impl<SPI: SpiInstance> Spi<SPI> {
    /// Construct the driver.
    ///
    /// - `clocks`: peripheral bus clock source.
    /// - `spi_hz`: target SCL frequency in Hz.
    /// - `polarity` / `phase`: SPI mode (CPOL / CPHA).
    /// - `bit_order`: MSB or LSB first.
    pub fn new(
        spi: SPI,
        clocks: &Clocks,
        spi_hz: u32,
        polarity: Polarity,
        phase: Phase,
        bit_order: BitOrder,
    ) -> Self {
        let driver = Self { _inner: spi };
        driver.configure(clocks.bclk, spi_hz, polarity, phase, bit_order);
        driver
    }

    fn regs(&self) -> &bl808_pac::spi0::RegisterBlock {
        unsafe { &*SPI::ptr() }
    }

    fn configure(
        &self,
        bclk_hz: u32,
        spi_hz: u32,
        polarity: Polarity,
        phase: Phase,
        bit_order: BitOrder,
    ) {
        let regs = self.regs();

        // Disable master before configuring.
        regs.spi_config()
            .modify(|_, w| w.cr_spi_m_en().clear_bit());

        // Clock divider: bit period = (ph_0 + ph_1 + 2) bclk cycles.
        // For 50% duty cycle: ph_0 = ph_1 = bclk / (2 * spi_hz) - 1.
        let half = (bclk_hz / (2 * spi_hz)).saturating_sub(1).min(0xff) as u8;

        regs.spi_prd_0().modify(|_, w| unsafe {
            w.cr_spi_prd_s()
                .bits(half)
                .cr_spi_prd_p()
                .bits(half)
                .cr_spi_prd_d_ph_0()
                .bits(half)
                .cr_spi_prd_d_ph_1()
                .bits(half)
        });

        // Mode and frame configuration.
        regs.spi_config().modify(|_, w| {
            w.cr_spi_sclk_pol()
                .bit(polarity == Polarity::IdleHigh)
                .cr_spi_sclk_ph()
                .bit(phase == Phase::CaptureOnSecondTransition)
                .cr_spi_bit_inv()
                .bit(bit_order == BitOrder::LsbFirst)
                // 8-bit frame (cr_spi_frame_size = 0).
                .cr_spi_rxd_ignr_en()
                .clear_bit()
                .cr_spi_m_cont_en()
                .clear_bit()
        });
        // Frame size 0 = 8-bit; write via unsafe bits since field may lack enum.
        regs.spi_config()
            .modify(|_, w| unsafe { w.cr_spi_frame_size().bits(0) });

        // Clear FIFOs.
        regs.spi_fifo_config_0().modify(|_, w| {
            w.tx_fifo_clr().clear_bit_by_one().rx_fifo_clr().clear_bit_by_one()
        });

        // Enable master.
        regs.spi_config()
            .modify(|_, w| w.cr_spi_m_en().set_bit());
    }

    /// Wait until the SPI bus is idle (all bytes shifted out).
    ///
    /// Returns `Err(SpiError::Timeout)` if the end flag never sets within
    /// `SPIN_BUDGET` iterations.
    fn wait_idle(&self) -> Result<(), SpiError> {
        let regs = self.regs();
        let mut budget = SPIN_BUDGET;
        // Wait for end interrupt (all data transmitted).
        while !regs.spi_int_sts().read().spi_end_int().bit_is_set() {
            if budget == 0 {
                return Err(SpiError::Timeout);
            }
            budget -= 1;
            core::hint::spin_loop();
        }
        // Clear the end flag.
        regs.spi_int_sts()
            .write(|w| w.cr_spi_end_clr().clear_bit_by_one());
        Ok(())
    }

    /// Push `word` into TX FIFO (blocking until space available).
    ///
    /// Returns `Err(SpiError::Timeout)` if the FIFO never becomes ready within
    /// `SPIN_BUDGET` iterations.
    #[inline]
    fn fifo_write_byte(&self, byte: u8) -> Result<(), SpiError> {
        let regs = self.regs();
        let mut budget = SPIN_BUDGET;
        loop {
            let cnt = regs.spi_fifo_config_1().read().tx_fifo_cnt().bits();
            if cnt > 0 {
                break;
            }
            if budget == 0 {
                return Err(SpiError::Timeout);
            }
            budget -= 1;
            core::hint::spin_loop();
        }
        regs.spi_fifo_wdata()
            .write(|w| unsafe { w.bits(byte as u32) });
        Ok(())
    }

    /// Pop one byte from RX FIFO (blocking until data available).
    ///
    /// Returns `Err(SpiError::Timeout)` if the FIFO never has data within
    /// `SPIN_BUDGET` iterations.
    #[inline]
    fn fifo_read_byte(&self) -> Result<u8, SpiError> {
        let regs = self.regs();
        let mut budget = SPIN_BUDGET;
        loop {
            let cnt = regs.spi_fifo_config_1().read().rx_fifo_cnt().bits();
            if cnt > 0 {
                break;
            }
            if budget == 0 {
                return Err(SpiError::Timeout);
            }
            budget -= 1;
            core::hint::spin_loop();
        }
        Ok(regs.spi_fifo_rdata().read().bits() as u8)
    }
}

// ---------------------------------------------------------------------------
// embedded-hal traits
// ---------------------------------------------------------------------------

impl<SPI: SpiInstance> ErrorType for Spi<SPI> {
    type Error = SpiError;
}

impl<SPI: SpiInstance> SpiBus<u8> for Spi<SPI> {
    /// Clock out `words` and discard the received bytes.
    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        for &byte in words {
            self.fifo_write_byte(byte)?;
            // Drain RX to prevent overflow (full-duplex hardware).
            let _ = self.fifo_read_byte()?;
        }
        Ok(())
    }

    /// Clock in `words.len()` bytes by transmitting dummy 0xFF bytes.
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        for slot in words.iter_mut() {
            self.fifo_write_byte(0xff)?;
            *slot = self.fifo_read_byte()?;
        }
        Ok(())
    }

    /// Full-duplex transfer: send `write[i]`, receive into `read[i]`.
    ///
    /// If `read` and `write` have different lengths the shorter side is
    /// padded with 0xFF (RX) or discarded (TX).
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        let len = read.len().max(write.len());
        for i in 0..len {
            let tx = write.get(i).copied().unwrap_or(0xff);
            self.fifo_write_byte(tx)?;
            let rx = self.fifo_read_byte()?;
            if let Some(slot) = read.get_mut(i) {
                *slot = rx;
            }
        }
        Ok(())
    }

    /// In-place full-duplex transfer (send and receive in the same buffer).
    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        for byte in words.iter_mut() {
            self.fifo_write_byte(*byte)?;
            *byte = self.fifo_read_byte()?;
        }
        Ok(())
    }

    /// Wait until the SPI controller is done shifting.
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.wait_idle()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::spi0::RegisterBlock;
    use crate::glb::Clocks;

    fn make_clocks() -> Clocks {
        Clocks { sys_clk: 40_000_000, hclk: 40_000_000, bclk: 40_000_000 }
    }

    #[test]
    fn test_spi_mode_cpol_cpha() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_SPI0_PTR.with(|p| p.set(mock_ptr));

        let spi0 = unsafe { Spi0::steal() };
        let _spi = Spi::new(
            spi0,
            &make_clocks(),
            1_000_000,
            Polarity::IdleHigh,
            Phase::CaptureOnSecondTransition,
            BitOrder::MsbFirst,
        );

        let cfg = unsafe { core::ptr::read_volatile(mock_ptr as *const u32) };
        // cr_spi_sclk_pol = bit 4, cr_spi_sclk_ph = bit 5
        assert_ne!(cfg & (1 << 4), 0, "CPOL should be set (idle high)");
        assert_ne!(cfg & (1 << 5), 0, "CPHA should be set (capture on second)");
    }

    #[test]
    fn test_spi_clock_divider() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_SPI0_PTR.with(|p| p.set(mock_ptr));

        let spi0 = unsafe { Spi0::steal() };
        // bclk = 40 MHz, spi_hz = 10 MHz → half = 40/(2*10) - 1 = 1
        let _spi = Spi::new(
            spi0,
            &make_clocks(),
            10_000_000,
            Polarity::IdleLow,
            Phase::CaptureOnFirstTransition,
            BitOrder::MsbFirst,
        );

        let regs = unsafe { &*mock_ptr };
        assert_eq!(
            regs.spi_prd_0().read().cr_spi_prd_d_ph_0().bits(),
            1,
            "prd_d_ph_0 should be 1 for 10 MHz SPI at 40 MHz bclk"
        );
    }

    #[test]
    fn test_spi_master_enable_on_new() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_SPI0_PTR.with(|p| p.set(mock_ptr));

        let spi0 = unsafe { Spi0::steal() };
        let _spi = Spi::new(
            spi0,
            &make_clocks(),
            1_000_000,
            Polarity::IdleLow,
            Phase::CaptureOnFirstTransition,
            BitOrder::MsbFirst,
        );

        let cfg = unsafe { core::ptr::read_volatile(mock_ptr as *const u32) };
        assert_ne!(cfg & 1, 0, "Master enable (bit 0) should be set after new()");
    }
}
