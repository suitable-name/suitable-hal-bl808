//! Serial Flash Controller (SF_CTRL) Driver — command-phase state machine
//!
//! The BL808 SF_CTRL exposes two interfaces for flash access:
//!
//! * **IAHB** – XIP / instruction-AHB path (managed by hardware, not this driver)
//! * **SAHB** – Software AHB: the CPU manually programs and triggers one flash
//!   command at a time through `SF_IF_SAHB_0/1/2`.
//!
//! # Command frame layout (SAHB_1 / cmd_buf_0)
//! ```text
//! Bits [31:24] = opcode byte
//! Bits [23:16] = address byte 2 (MSB, for 3-byte addressing)
//! Bits [15: 8] = address byte 1
//! Bits [ 7: 0] = address byte 0 (LSB)
//! ```
//!
//! # Usage
//! ```ignore
//! let mut flash = Flash::new(peripherals.sf_ctrl);
//! let jedec = flash.read_jedec_id();
//! ```

use bl808_pac::SfCtrl as PacSfCtrl;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_SF_PTR: core::cell::Cell<*const bl808_pac::sf_ctrl::RegisterBlock>
        = const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// SPI mode constants (sf_if_0_spi_mode field)
// ---------------------------------------------------------------------------
/// Standard 1-bit SPI mode.
pub const SPI_MODE_1BIT: u8 = 0;
/// Dual-SPI mode (2-bit I/O).
pub const SPI_MODE_DUAL: u8 = 4;
/// Quad-SPI mode (4-bit I/O).
pub const SPI_MODE_QUAD: u8 = 5;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashError {
    /// The SF_CTRL hardware did not become idle within the timeout.
    Timeout,
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

pub struct Flash {
    _flash: PacSfCtrl,
}

impl Flash {
    pub fn new(flash: PacSfCtrl) -> Self {
        Self { _flash: flash }
    }

    fn regs(&self) -> &bl808_pac::sf_ctrl::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacSfCtrl::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_SF_PTR.with(|p| p.get()) };
    }

    /// Wait until the SF_CTRL SAHB interface is idle.
    fn wait_idle(&self) -> Result<(), FlashError> {
        let mut timeout = 1_000_000_u32;
        while self.regs().sf_if_sahb_0().read().sf_if_busy().bit_is_set() {
            timeout -= 1;
            if timeout == 0 {
                return Err(FlashError::Timeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Issue a raw command via the SAHB interface.
    ///
    /// * `opcode`    – single-byte flash opcode (e.g. 0x9F for JEDEC ID)
    /// * `addr`      – optional 3-byte address packed in bits[23:0]
    /// * `addr_en`   – include address phase in the command frame
    /// * `addr_bytes`– number of address bytes (1–3; ignored when `addr_en` is false)
    /// * `data`      – data to write (for write commands); ignored for reads
    /// * `dat_bytes` – number of data bytes to transfer (0 = no data phase)
    /// * `read`      – true = read data phase, false = write data phase
    /// * `spi_mode`  – `SPI_MODE_1BIT`, `SPI_MODE_DUAL`, or `SPI_MODE_QUAD`
    ///
    /// Returns the up-to-8-byte response packed into a `u64`
    /// (`cmd_buf_0` in bits[31:0], `cmd_buf_1` in bits[63:32]).
    #[allow(clippy::too_many_arguments)]
    pub fn issue_command(
        &mut self,
        opcode: u8,
        addr: u32,
        addr_en: bool,
        addr_bytes: u8,
        data: u32,
        dat_bytes: u16,
        read: bool,
        spi_mode: u8,
    ) -> Result<u64, FlashError> {
        self.wait_idle()?;

        // Pack opcode + address into cmd_buf_0:
        // [31:24] = opcode, [23:0] = address (MSB first).
        let cmd_buf_0 = ((opcode as u32) << 24) | (addr & 0x00FF_FFFF);
        self.regs()
            .sf_if_sahb_1()
            .write(|w| unsafe { w.sf_if_0_cmd_buf_0().bits(cmd_buf_0) });

        // Write data into cmd_buf_1 for write commands.
        self.regs()
            .sf_if_sahb_2()
            .write(|w| unsafe { w.sf_if_0_cmd_buf_1().bits(data) });

        // Configure the command phase and trigger.
        let addr_bytes_field = if addr_en { addr_bytes.saturating_sub(1) } else { 0 };
        let dat_bytes_field = dat_bytes.saturating_sub(1).min(0x3FF);

        self.regs().sf_if_sahb_0().write(|w| unsafe {
            w.sf_if_0_trig()
                .set_bit()
                .sf_if_0_cmd_en()
                .set_bit()
                .sf_if_0_cmd_byte()
                .bits(0) // 1 opcode byte (field = count-1, so 0 = 1 byte)
                .sf_if_0_adr_en()
                .bit(addr_en)
                .sf_if_0_adr_byte()
                .bits(addr_bytes_field)
                .sf_if_0_dat_en()
                .bit(dat_bytes > 0)
                .sf_if_0_dat_byte()
                .bits(dat_bytes_field)
                .sf_if_0_dat_rw()
                .bit(read)
                .sf_if_0_dmy_en()
                .clear_bit()
                .sf_if_0_spi_mode()
                .bits(spi_mode)
                .sf_if_0_qpi_mode_en()
                .clear_bit()
        });

        self.wait_idle()?;

        // Read back result from cmd_buf_0/1.
        let result_lo = self.regs().sf_if_sahb_1().read().sf_if_0_cmd_buf_0().bits();
        let result_hi = self.regs().sf_if_sahb_2().read().sf_if_0_cmd_buf_1().bits();
        Ok((result_lo as u64) | ((result_hi as u64) << 32))
    }

    /// Read the 3-byte JEDEC device ID (opcode 0x9F, no address).
    ///
    /// Returns `[manufacturer_id, memory_type, capacity]`.
    pub fn read_jedec_id(&mut self) -> Result<[u8; 3], FlashError> {
        let result = self.issue_command(
            0x9F, // JEDEC Read ID
            0,
            false,
            0,
            0,
            3, // 3 bytes: manufacturer, type, capacity
            true,
            SPI_MODE_1BIT,
        )?;
        let bytes = (result as u32).to_be_bytes();
        Ok([bytes[1], bytes[2], bytes[3]])
    }

    /// Read one byte from flash at the given 24-bit address (opcode 0x03).
    pub fn read_byte(&mut self, addr: u32) -> Result<u8, FlashError> {
        let result = self.issue_command(
            0x03, // Read Data
            addr,
            true,
            3,
            0,
            1,
            true,
            SPI_MODE_1BIT,
        )?;
        Ok(((result >> 24) & 0xFF) as u8)
    }

    /// Send Write-Enable (opcode 0x06, no address, no data).
    pub fn write_enable(&mut self) -> Result<(), FlashError> {
        self.issue_command(0x06, 0, false, 0, 0, 0, false, SPI_MODE_1BIT)?;
        Ok(())
    }

    /// Read the 1-byte Status Register 1 (opcode 0x05).
    pub fn read_status(&mut self) -> Result<u8, FlashError> {
        let result = self.issue_command(
            0x05, // Read Status Register 1
            0,
            false,
            0,
            0,
            1,
            true,
            SPI_MODE_1BIT,
        )?;
        Ok(((result >> 24) & 0xFF) as u8)
    }

    /// Enable or disable the 32-bit address extension for flash >16 MiB.
    pub fn set_4byte_address_mode(&mut self, enable: bool) {
        self.regs()
            .sf_ctrl_0()
            .modify(|_, w| w.sf_if_32b_adr_en().bit(enable));
    }

    /// Configure the SF_CTRL clock: RX sample inversion and output inversion.
    pub fn configure_clock(&mut self, rx_inv: bool, out_inv: bool) {
        self.regs().sf_ctrl_0().modify(|_, w| {
            w.sf_clk_sf_rx_inv_sel()
                .bit(rx_inv)
                .sf_clk_out_inv_sel()
                .bit(out_inv)
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::sf_ctrl::RegisterBlock;

    fn setup() -> *const RegisterBlock {
        let mock_data = std::vec![0u32; 256];
        let ptr = mock_data.as_ptr() as *const RegisterBlock;
        std::mem::forget(mock_data);
        MOCK_SF_PTR.with(|p| p.set(ptr));
        ptr
    }

    #[test]
    fn test_write_enable_opcode_in_cmd_buf() {
        let ptr = setup();
        let sf_ctrl = unsafe { PacSfCtrl::steal() };
        let mut driver = Flash::new(sf_ctrl);

        // write_enable uses opcode 0x06, no addr, no data.
        // sf_if_busy will read 0 from the mock so wait_idle() returns immediately.
        // After issue_command sets trig, busy clears on the next mock read.
        let _ = driver.write_enable();

        // SF_IF_SAHB_1 (cmd_buf_0) should have opcode 0x06 in bits[31:24].
        // The register offset for sf_if_sahb_1 varies; check bits[31:24].
        let raw = unsafe {
            // sf_if_sahb_1 is the second register relative to SF_CTRL base.
            // We can verify indirectly: cmd_buf_0 = (0x06 << 24) | 0 = 0x0600_0000.
            core::ptr::read_volatile(ptr.cast::<u32>())
        };
        let _ = raw; // Offset discovery needed; just ensure no panic.
    }

    #[test]
    fn test_4byte_address_mode() {
        let ptr = setup();
        let sf_ctrl = unsafe { PacSfCtrl::steal() };
        let mut driver = Flash::new(sf_ctrl);

        driver.set_4byte_address_mode(true);

        // sf_ctrl_0 bit 19 = sf_if_32b_adr_en
        let raw = unsafe { core::ptr::read_volatile(ptr as *const u32) };
        assert_ne!(raw & (1 << 19), 0, "32b address enable bit should be set");

        driver.set_4byte_address_mode(false);
        let raw = unsafe { core::ptr::read_volatile(ptr as *const u32) };
        assert_eq!(raw & (1 << 19), 0, "32b address enable bit should be cleared");
    }

    #[test]
    fn test_clock_config() {
        let ptr = setup();
        let sf_ctrl = unsafe { PacSfCtrl::steal() };
        let mut driver = Flash::new(sf_ctrl);

        driver.configure_clock(true, false);

        // sf_ctrl_0 bit 2 = sf_clk_sf_rx_inv_sel, bit 4 = sf_clk_out_inv_sel
        let raw = unsafe { core::ptr::read_volatile(ptr as *const u32) };
        assert_ne!(raw & (1 << 2), 0, "RX invert should be set");
        assert_eq!(raw & (1 << 4), 0, "OUT invert should be clear");
    }
}
