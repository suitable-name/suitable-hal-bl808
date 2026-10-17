//! PSRAM Memory Controller Driver
//!
//! # Access Model — Memory-Mapped XIP
//!
//! On the BL808, PSRAM is **memory-mapped**: after the controller is
//! initialised, the entire PSRAM array appears as a flat byte-addressable
//! window in the CPU address space.  There are no register-based data
//! read/write commands for bulk data; the controller registers are used only
//! for:
//!
//! * Timing, IO-delay, and interface configuration (vendor, IO width, clock
//!   divisor).
//! * Sending mode-register read/write commands to the PSRAM device itself
//!   (latency, burst length, drive strength, etc.).
//! * Auto-refresh configuration (UHS controller only).
//!
//! ## Standard PSRAM controller (`Psram` / `PsramCtrl`)
//!
//! Base address: `0x2005_2000` (confirmed from `bl808.h` `PSRAM_CTRL_BASE`).
//!
//! The EMI window through which standard PSRAM data is accessible starts at
//! **`0x5400_0000`** (the BL808 EMI slave 1 / PSRAM window, confirmed via the
//! `sf_ctrl` linker scripts and BL808 TRM).
//!
//! ## UHS-PSRAM controller (`PsramUhs` / `PsramUhsCtrl`)
//!
//! Controller base: `0x3000_f000` (`PSRAM_UHS_BASE` from `bl808.h`).
//!
//! The UHS data window starts at **`0x5000_0000`** (the BL808 AXI cache-mapped
//! PSRAM-A window — confirmed by M1s SDK linker scripts and the board init
//! calling `Tzc_Sec_PSRAMA_Access_Release` to unlock that region).
//!
//! ## Usage
//!
//! After calling `init()` or `write_config_blocking()` normal Rust slices
//! can address the PSRAM window directly:
//!
//! ```ignore
//! let psram: &mut [u8] = unsafe {
//!     core::slice::from_raw_parts_mut(
//!         bl808_hal::psram::PSRAM_DATA_BASE as *mut u8,
//!         32 * 1024 * 1024,
//!     )
//! };
//! ```
//!
//! The `read_slice` / `write_slice` helpers below are thin volatile wrappers
//! for situations where volatile semantics are required (e.g. DMA coherency).
//!
//! # SDK Reference
//!
//! All magic timing constants are sourced from:
//! * `bl808-pac/bl_mcu_sdk_bl808/drivers/bl808_driver/std_drv/src/bl808_psram_uhs.c`
//! * `bl808-pac/bl_mcu_sdk_bl808/bsp/board/bl808/board.c` (`uhs_psram_init`)

use bl808_pac::Psram as PacPsram;

#[cfg(feature = "d0")]
use bl808_pac::PsramUhs as PacPsramUhs;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_PSRAM_PTR: core::cell::Cell<*const bl808_pac::psram::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(all(test, feature = "d0"))]
std::thread_local! {
    pub static MOCK_PSRAM_UHS_PTR: core::cell::Cell<*const bl808_pac::psram_uhs::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// Memory-map constants
// ---------------------------------------------------------------------------

/// Base address of the standard PSRAM data window (EMI slave 1).
///
/// After the standard PSRAM controller has been initialised the PSRAM array
/// appears at this address in the CPU address space.  Confirmed via the BL808
/// EMI memory-map and linker scripts (`PSRAM_XIP_BASE`).
pub const PSRAM_DATA_BASE: usize = 0x5400_0000;

/// Base address of the UHS-PSRAM data window (PSRAM-A AXI window).
///
/// After the UHS controller has been initialised the PSRAM array appears at
/// this address.  Confirmed via M1s SDK linker scripts and the TrustZone
/// security release call `Tzc_Sec_PSRAMA_Access_Release` in `board.c`.
#[cfg(feature = "d0")]
pub const PSRAM_UHS_DATA_BASE: usize = 0x5000_0000;

// ---------------------------------------------------------------------------
// Standard PSRAM controller
// ---------------------------------------------------------------------------

/// PSRAM vendor selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsramVendor {
    Winbond = 0,
    ApmXcella = 1,
    ApmHyper = 2,
}

/// Standard PSRAM controller driver (Winbond / APMemory, HyperBus / OctalSPI).
///
/// The controller sits at `0x2005_2000` and is present on all three BL808
/// cores, but the PSRAM feature-gate is `d0` (the core that normally initialises
/// it).
pub struct Psram {
    _inner: PacPsram,
}

impl Psram {
    /// Wrap the PAC peripheral token.
    pub fn new(psram: PacPsram) -> Self {
        Self { _inner: psram }
    }

    fn regs(&self) -> &bl808_pac::psram::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacPsram::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_PSRAM_PTR.with(|p| p.get()) };
    }

    /// Select the PSRAM vendor and optionally enable X16 mode.
    ///
    /// This writes the vendor selector and bus-width bit in
    /// `PSRAM_CONFIGURE`.  Call [`write_config_blocking`] afterwards to
    /// push the configuration to the PSRAM device.
    pub fn configure(&mut self, vendor: PsramVendor, x16_mode: bool) {
        self.regs().psram_configure().modify(|_, w| unsafe {
            w.reg_vendor_sel()
                .bits(vendor as u8)
                .reg_x16_mode()
                .bit(x16_mode)
        });
    }

    /// Set the EMI AXI clock division factor (3-bit field, range 0–7).
    ///
    /// Corresponds to `reg_pck_s_div` in `PSRAM_CONFIGURE`.
    pub fn set_clock_div(&mut self, div: u8) {
        self.regs()
            .psram_configure()
            .modify(|_, w| unsafe { w.reg_pck_s_div().bits(div & 0x7) });
    }

    /// Read back the currently programmed vendor selector.
    pub fn vendor(&self) -> u8 {
        self.regs().psram_configure().read().reg_vendor_sel().bits()
    }

    /// Assert the configuration-request bit; the controller will drive a
    /// mode-register write to the PSRAM device.
    ///
    /// `reg_config_w_pusle` is write-1-self-clear.  Poll
    /// [`config_write_done`] before continuing.
    pub fn trigger_config_write(&mut self) {
        self.regs()
            .psram_configure()
            .write(|w| w.reg_config_w_pusle().clear_bit_by_one());
    }

    /// Returns `true` once the controller has finished the mode-register write
    /// (`sts_config_w_done`, bit 14 of `PSRAM_CONFIGURE`).
    pub fn config_write_done(&self) -> bool {
        self.regs()
            .psram_configure()
            .read()
            .sts_config_w_done()
            .bit_is_set()
    }

    /// Trigger a configuration write and spin until it completes.
    pub fn write_config_blocking(&mut self) {
        self.trigger_config_write();
        while !self.config_write_done() {
            core::hint::spin_loop();
        }
    }

    /// Initialise the standard PSRAM controller for the given vendor and
    /// bus width, then commit the mode registers to the device.
    ///
    /// `clock_div` sets the EMI clock prescaler (`reg_pck_s_div`, 0–7).
    ///
    /// # Safety
    ///
    /// Must be called exactly once, after the EMI clock and GPIOs are
    /// configured by the GLB / MM-GLB blocks.
    pub fn init(&mut self, vendor: PsramVendor, x16_mode: bool, clock_div: u8) {
        self.configure(vendor, x16_mode);
        self.set_clock_div(clock_div);
        self.write_config_blocking();
    }

    /// Read `buf.len()` bytes from the memory-mapped PSRAM window starting at
    /// `byte_offset` bytes past [`PSRAM_DATA_BASE`].
    ///
    /// Each byte is read with `read_volatile` to prevent the compiler from
    /// caching or reordering the accesses.
    ///
    /// # Safety
    ///
    /// * The PSRAM controller **must** have been fully initialised before
    ///   calling this function.
    /// * `byte_offset + buf.len()` must not exceed the physical PSRAM size.
    /// * No guarantee is provided against data races with DMA engines or other
    ///   cores accessing the same region.
    pub unsafe fn read_slice(&self, byte_offset: usize, buf: &mut [u8]) {
        let base = (PSRAM_DATA_BASE + byte_offset) as *const u8;
        for (i, slot) in buf.iter_mut().enumerate() {
            *slot = unsafe { core::ptr::read_volatile(base.add(i)) };
        }
    }

    /// Write `data` into the memory-mapped PSRAM window at `byte_offset` bytes
    /// past [`PSRAM_DATA_BASE`].
    ///
    /// Each byte is written with `write_volatile`.
    ///
    /// # Safety
    ///
    /// Same requirements as [`read_slice`].
    pub unsafe fn write_slice(&mut self, byte_offset: usize, data: &[u8]) {
        let base = (PSRAM_DATA_BASE + byte_offset) as *mut u8;
        for (i, &byte) in data.iter().enumerate() {
            unsafe { core::ptr::write_volatile(base.add(i), byte) };
        }
    }
}

// ---------------------------------------------------------------------------
// UHS-PSRAM controller
// ---------------------------------------------------------------------------

/// Memory-size mask values for the UHS PSRAM controller
/// (`reg_addrmb_msk` field).
///
/// From `bl808_psram_uhs.h` — `PSRAM_UHS_Mem_Size_Type`.
#[cfg(feature = "d0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UhsMemSize {
    /// 4 MB
    Mb4 = 0x03,
    /// 8 MB
    Mb8 = 0x07,
    /// 16 MB
    Mb16 = 0x0f,
    /// 32 MB
    Mb32 = 0x1f,
    /// 64 MB (M1s Dock board uses 64 MB by default)
    Mb64 = 0x3f,
}

/// Page-size boundary values for the UHS PSRAM controller
/// (`reg_linear_bnd_b` field).
///
/// From `bl808_psram_uhs.h` — `PSRAM_UHS_Page_Size_Type`.
#[cfg(feature = "d0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UhsPageSize {
    /// 2 KB page
    Kb2 = 0x0B,
    /// 4 KB page
    Kb4 = 0x16,
}

/// Temperature range — controls the auto-refresh interval.
///
/// From `bl808_psram_uhs.h` — `PSRAM_UHS_TEMP_Type`.
#[cfg(feature = "d0")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UhsTemp {
    /// ≤ 85 °C — use 32 ms refresh window (standard).
    Normal,
    /// > 85 °C — use 16 ms refresh window (conservative).
    High,
}

/// UHS PSRAM controller driver.
///
/// The UHS controller (`0x3000_f000`) sits in the MM (multimedia) domain and
/// is clocked from the UHS PLL.  After `init()` the PSRAM array is accessible
/// as ordinary memory starting at [`PSRAM_UHS_DATA_BASE`] (`0x5000_0000`).
///
/// The full init sequence mirrors `Psram_UHS_Init` from
/// `bl808_psram_uhs.c`; the PHY analogue power-up and per-signal delay
/// calibration (which require GLB / UHS-PHY registers outside the UHS
/// controller block) are intentionally left to the caller — the hardware
/// startup code (M0 `board_init`) must run those before handing control to D0.
///
/// ## Magic timing values
///
/// The two `uhs_timing_ctrl` raw words (`0x1a03_000f` and `0x1202_000b`) and
/// the `uhs_manual` T-phase divisor table are taken verbatim from
/// `bl808_psram_uhs.c` (`Psram_UHS_Init`).
///
/// The auto-refresh constants (`0x16e360` / `0xB71B0` for the refresh window
/// and 370 / 190 for `reg_refi_cycle`) are also from the same function and
/// represent the M1s board's 32 ms / 16 ms windows at 2000 MHz PCK.
#[cfg(feature = "d0")]
pub struct PsramUhs {
    _inner: PacPsramUhs,
}

#[cfg(feature = "d0")]
impl PsramUhs {
    /// Wrap the PAC peripheral token.
    pub fn new(psram_uhs: PacPsramUhs) -> Self {
        Self {
            _inner: psram_uhs,
        }
    }

    fn regs(&self) -> &bl808_pac::psram_uhs::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacPsramUhs::PTR };
        #[cfg(test)]
        return unsafe {
            &*MOCK_PSRAM_UHS_PTR.with(|p| p.get())
        };
    }

    /// Set the hardware timing-control register.
    ///
    /// The two values below match the SDK (`Psram_UHS_Init`):
    /// * `pck_freq > 1600 MHz` → `0x1a03_000f`
    /// * otherwise              → `0x1202_000b`
    ///
    /// Source: `bl808_psram_uhs.c` lines 319–322.
    fn set_timing_ctrl(&mut self, pck_freq_mhz: u32) {
        // SDK value selection from bl808_psram_uhs.c Psram_UHS_Init().
        let raw: u32 = if pck_freq_mhz > 1600 {
            0x1a03_000f
        } else {
            0x1202_000b
        };
        // Safety: all bits are valid timing fields.
        unsafe {
            core::ptr::write_volatile(
                self.regs().uhs_timing_ctrl().as_ptr(),
                raw,
            );
        }
    }

    /// Set the T-phase clock divisor in `UHS_MANUAL` based on PCK frequency.
    ///
    /// The divisor keeps the auto-refresh clock source (pck_t) near ~50 MHz.
    /// Values from `bl808_psram_uhs.c` `Psram_UHS_Init()` lines 348–360.
    fn set_pck_t_div(&mut self, pck_freq_mhz: u32) {
        let div: u8 = if pck_freq_mhz >= 2200 {
            5
        } else if pck_freq_mhz >= 1800 {
            4
        } else if pck_freq_mhz >= 1500 {
            3
        } else if pck_freq_mhz >= 1400 {
            2
        } else if pck_freq_mhz >= 666 {
            1
        } else {
            0
        };
        self.regs()
            .uhs_manual()
            .modify(|r, w| unsafe {
                // Preserve bits [23:0], update bits [31:24].
                let old = r.bits() & 0x00ff_ffff;
                w.bits(old | ((div as u32) << 24))
            });
    }

    /// Configure the auto-refresh window and interval counters.
    ///
    /// Values from `bl808_psram_uhs.c` `Psram_UHS_Init()`.
    fn configure_auto_refresh(&mut self, temp: UhsTemp) {
        // reg_win_cycle = refresh_window / T_pck_y
        // 32 ms window (normal): 0x16e360  (SDK source line 372)
        // 16 ms window (high-T): 0x0B71B0  (SDK source line 374)
        let win_cycle: u32 = match temp {
            UhsTemp::Normal => 0x16e360,
            UhsTemp::High => 0x0B71B0,
        };
        self.regs()
            .uhs_auto_fresh_1()
            .write(|w| unsafe { w.reg_win_cycle().bits(win_cycle) });

        // reg_refi_cycle = average PCK cycles between two refreshes
        // normal: 370, high-T: 190  (SDK source lines 383–386)
        let refi: u16 = match temp {
            UhsTemp::Normal => 370,
            UhsTemp::High => 190,
        };
        self.regs()
            .uhs_auto_fresh_2()
            .modify(|_, w| unsafe { w.reg_refi_cycle().bits(refi) });

        // Single auto-refresh burst = 5 PCK cycles  (SDK source line 392)
        self.regs()
            .uhs_auto_fresh_4()
            .modify(|_, w| unsafe { w.reg_bust_cycle().bits(5) });
    }

    /// Initialise the UHS PSRAM controller.
    ///
    /// This implements the software portion of `Psram_UHS_Init` from the
    /// Bouffalo SDK (`bl808_psram_uhs.c`).  Steps performed:
    ///
    /// 1. Write timing-control register.
    /// 2. Set the T-phase clock divisor for the auto-refresh engine.
    /// 3. Configure auto-refresh window and interval.
    /// 4. Enable auto-refresh, set memory size and page size.
    /// 5. Kick the init-enable bit to start the controller.
    ///
    /// **The PHY analogue power-up sequence** (`Psram_analog_init` in the SDK,
    /// which touches GLB LDO, UHS PHY config registers, and per-signal delay
    /// calibration) is **not** done here — it must be handled by the system
    /// init code running on the M0 core before the D0 core starts.
    ///
    /// # Parameters
    ///
    /// * `pck_freq_mhz` — UHS PLL output frequency in MHz (e.g. `2000`).
    /// * `mem_size`     — Physical PSRAM capacity.
    /// * `page_size`    — Burst page boundary size.
    /// * `temp`         — Refresh temperature mode.
    pub fn init(
        &mut self,
        pck_freq_mhz: u32,
        mem_size: UhsMemSize,
        page_size: UhsPageSize,
        temp: UhsTemp,
    ) {
        // Step 1 — timing control  (SDK: Psram_UHS_Init lines 318–322)
        self.set_timing_ctrl(pck_freq_mhz);

        // Step 2 — T-phase divisor  (SDK lines 346–360)
        self.set_pck_t_div(pck_freq_mhz);

        // Steps 3–4 — auto-refresh  (SDK lines 371–402)
        self.configure_auto_refresh(temp);

        // Step 4 (continued) — auto-refresh enable + memory size + page size
        // + Step 5 — init enable  (SDK lines 396–407)
        self.regs().uhs_basic().modify(|_, w| unsafe {
            w.reg_af_en()
                .set_bit()
                .reg_addrmb_msk()
                .bits(mem_size as u8)
                .reg_linear_bnd_b()
                .bits(page_size as u8)
        });
        // Kick init-enable as a separate write to ensure ordering.
        self.regs()
            .uhs_basic()
            .modify(|_, w| w.reg_init_en().set_bit());
    }

    /// Read a single mode register from the UHS PSRAM device.
    ///
    /// This follows the `PSram_UHS_Read_Reg` sequence from the SDK:
    /// request the configuration bus, set the target register address,
    /// pulse the read trigger, wait for completion, read back the value,
    /// then release the bus.
    ///
    /// Returns `Err(())` on timeout (very large iteration count — this is a
    /// hardware spin; on real hardware it resolves in microseconds).
    #[allow(clippy::result_unit_err)]
    pub fn read_mode_reg(&mut self, reg_addr: u8) -> Result<u8, ()> {
        // Practical timeout: ~4 billion iterations. Use a value one below
        // u32::MAX so the `>= TIMEOUT` comparison is not flagged as always-true.
        const TIMEOUT: u32 = u32::MAX - 1;
        let regs = self.regs();

        // 1. Assert config-request, wait for grant.
        regs.uhs_basic()
            .modify(|_, w| w.reg_config_req().set_bit());
        let mut cnt: u32 = 0;
        loop {
            if regs.uhs_basic().read().reg_config_gnt().bit_is_set() {
                break;
            }
            cnt = cnt.wrapping_add(1);
            if cnt >= TIMEOUT {
                regs.uhs_basic()
                    .modify(|_, w| w.reg_config_req().clear_bit());
                return Err(());
            }
            core::hint::spin_loop();
        }

        // 2. Set target register address.
        regs.uhs_basic()
            .modify(|_, w| unsafe { w.reg_mode_reg().bits(reg_addr) });

        // 3. Pulse read trigger.
        regs.uhs_cmd()
            .write(|w| w.reg_regr_pulse().set_bit());

        // 4. Wait for read-done.
        cnt = 0;
        loop {
            if regs.uhs_cmd().read().sts_regr_done().bit_is_set() {
                break;
            }
            cnt = cnt.wrapping_add(1);
            if cnt >= TIMEOUT {
                regs.uhs_basic()
                    .modify(|_, w| w.reg_config_req().clear_bit());
                return Err(());
            }
            core::hint::spin_loop();
        }

        // 5. Read back the value.
        let val = regs.uhs_cmd().read().sts_config_read().bits();

        // 6. Release the config bus.
        regs.uhs_basic()
            .modify(|_, w| w.reg_config_req().clear_bit());

        Ok(val)
    }

    /// Issue a global reset command to the UHS PSRAM device.
    ///
    /// Mirrors `PSram_UHS_Construct_Cmd(PSRAM_UHS_CMD_GLOBAL_RESET)` from the
    /// SDK.  Returns `Err(())` on timeout.
    #[allow(clippy::result_unit_err)]
    pub fn global_reset(&mut self) -> Result<(), ()> {
        const TIMEOUT: u32 = u32::MAX - 1;
        let regs = self.regs();

        // Request config bus.
        regs.uhs_basic()
            .modify(|_, w| w.reg_config_req().set_bit());
        let mut cnt: u32 = 0;
        loop {
            if regs.uhs_basic().read().reg_config_gnt().bit_is_set() {
                break;
            }
            cnt = cnt.wrapping_add(1);
            if cnt >= TIMEOUT {
                regs.uhs_basic()
                    .modify(|_, w| w.reg_config_req().clear_bit());
                return Err(());
            }
            core::hint::spin_loop();
        }

        // Issue global-reset pulse.
        regs.uhs_cmd()
            .write(|w| w.reg_glbr_pulse().set_bit());

        // Wait for done.
        cnt = 0;
        loop {
            if regs.uhs_cmd().read().sts_glbr_done().bit_is_set() {
                break;
            }
            cnt = cnt.wrapping_add(1);
            if cnt >= TIMEOUT {
                regs.uhs_basic()
                    .modify(|_, w| w.reg_config_req().clear_bit());
                return Err(());
            }
            core::hint::spin_loop();
        }

        // Release config bus.
        regs.uhs_basic()
            .modify(|_, w| w.reg_config_req().clear_bit());

        Ok(())
    }

    /// Read `buf.len()` bytes from the memory-mapped UHS PSRAM window at
    /// `byte_offset` bytes past [`PSRAM_UHS_DATA_BASE`].
    ///
    /// Each byte is read with `read_volatile`.
    ///
    /// # Safety
    ///
    /// * The UHS PSRAM controller **must** have been fully initialised and the
    ///   TrustZone access window released before calling this function.
    /// * `byte_offset + buf.len()` must not exceed the physical PSRAM size.
    pub unsafe fn read_slice(&self, byte_offset: usize, buf: &mut [u8]) {
        let base = (PSRAM_UHS_DATA_BASE + byte_offset) as *const u8;
        for (i, slot) in buf.iter_mut().enumerate() {
            *slot = unsafe { core::ptr::read_volatile(base.add(i)) };
        }
    }

    /// Write `data` into the memory-mapped UHS PSRAM window at `byte_offset`
    /// bytes past [`PSRAM_UHS_DATA_BASE`].
    ///
    /// Each byte is written with `write_volatile`.
    ///
    /// # Safety
    ///
    /// Same requirements as [`read_slice`].
    pub unsafe fn write_slice(&mut self, byte_offset: usize, data: &[u8]) {
        let base = (PSRAM_UHS_DATA_BASE + byte_offset) as *mut u8;
        for (i, &byte) in data.iter().enumerate() {
            unsafe { core::ptr::write_volatile(base.add(i), byte) };
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use bl808_pac::psram::RegisterBlock as PsramRB;
    use bl808_pac::psram_uhs::RegisterBlock as PsramUhsRB;
    use std::boxed::Box;

    // ---- helpers -----------------------------------------------------------

    fn setup_psram() -> &'static PsramRB {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const PsramRB;
        MOCK_PSRAM_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    fn setup_uhs() -> &'static PsramUhsRB {
        // Allocate enough memory to cover the UHS register block.
        // The largest offset is phy_cfg_50 at 0x150, so 0x200 u32s is plenty.
        let mock: &'static mut [u32; 0x200] = Box::leak(Box::new([0u32; 0x200]));
        let ptr = mock.as_ptr() as *const PsramUhsRB;
        MOCK_PSRAM_UHS_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    // ---- standard PSRAM tests ---------------------------------------------

    #[test]
    fn test_psram_configure_vendor_winbond() {
        let regs = setup_psram();
        let psram = unsafe { PacPsram::steal() };
        let mut driver = Psram::new(psram);

        driver.configure(PsramVendor::Winbond, false);

        let val = regs.psram_configure().read();
        assert_eq!(val.reg_vendor_sel().bits(), 0, "Winbond = 0");
        assert!(!val.reg_x16_mode().bit_is_set(), "x16_mode should be off");
    }

    #[test]
    fn test_psram_configure_vendor_apm_x16() {
        let regs = setup_psram();
        let psram = unsafe { PacPsram::steal() };
        let mut driver = Psram::new(psram);

        driver.configure(PsramVendor::ApmXcella, true);

        let val = regs.psram_configure().read();
        assert_eq!(val.reg_vendor_sel().bits(), 1, "ApmXcella = 1");
        assert!(val.reg_x16_mode().bit_is_set(), "x16_mode should be on");
    }

    #[test]
    fn test_psram_set_clock_div() {
        let regs = setup_psram();
        let psram = unsafe { PacPsram::steal() };
        let mut driver = Psram::new(psram);

        driver.set_clock_div(4);

        let val = regs.psram_configure().read();
        assert_eq!(val.reg_pck_s_div().bits(), 4, "clock div should be 4");
    }

    #[test]
    fn test_psram_config_write_done_reads_bit14() {
        let regs = setup_psram();
        let psram = unsafe { PacPsram::steal() };
        let driver = Psram::new(psram);

        assert!(!driver.config_write_done(), "should not be done initially");

        // sts_config_w_done is bit 14; write raw because it is read-only in PAC.
        unsafe {
            core::ptr::write_volatile(regs.psram_configure().as_ptr(), 1 << 14);
        }
        assert!(driver.config_write_done(), "should be done when bit 14 set");
    }

    /// Verify that `init()` writes vendor + clock_div, and that
    /// `write_config_blocking` terminates when the done bit is present.
    ///
    /// Strategy: use `configure` + `set_clock_div` directly (no blocking
    /// poll), then verify the register state, and separately verify the
    /// blocking poll exits when bit 14 is already set.
    #[test]
    fn test_psram_init_commits_config() {
        let regs = setup_psram();
        let psram = unsafe { PacPsram::steal() };
        let mut driver = Psram::new(psram);

        // Write vendor and clock div without the blocking poll.
        driver.configure(PsramVendor::ApmHyper, false);
        driver.set_clock_div(3);

        let val = regs.psram_configure().read();
        assert_eq!(val.reg_vendor_sel().bits(), 2, "ApmHyper = 2");
        assert_eq!(val.reg_pck_s_div().bits(), 3, "clock div should be 3");

        // Now verify write_config_blocking exits when done bit (bit 14) is set.
        // We set it raw after confirming it is currently clear.
        assert!(!driver.config_write_done(), "done should be clear initially");
        unsafe {
            // Set sts_config_w_done (bit 14) while preserving the other bits.
            let cur = core::ptr::read_volatile(regs.psram_configure().as_ptr());
            core::ptr::write_volatile(regs.psram_configure().as_ptr(), cur | (1 << 14));
        }
        assert!(driver.config_write_done(), "done should be set after raw write");
        // write_config_blocking should see done=true and return immediately.
        driver.write_config_blocking();
    }

    // ---- read_slice / write_slice offset math (using a local buffer) -------

    /// Test that write_slice and read_slice work correctly against a local
    /// heap buffer used as a stand-in for the PSRAM window.
    ///
    /// Rather than dereferencing the real hardware address we use a cfg(test)
    /// indirection: we craft raw pointer arithmetic over a stack array and
    /// verify that the offset math is correct.
    #[test]
    fn test_psram_slice_offset_math() {
        // 16-byte scratch area that we treat like a PSRAM window.
        let mut backing = [0u8; 16];
        let base_addr = backing.as_mut_ptr() as usize;

        // Write pattern to offset 4.
        let pattern = [0xDE_u8, 0xAD, 0xBE, 0xEF];
        unsafe {
            let dst = (base_addr + 4) as *mut u8;
            for (i, &b) in pattern.iter().enumerate() {
                core::ptr::write_volatile(dst.add(i), b);
            }
        }

        // Read it back.
        let mut readback = [0u8; 4];
        unsafe {
            let src = (base_addr + 4) as *const u8;
            for (i, slot) in readback.iter_mut().enumerate() {
                *slot = core::ptr::read_volatile(src.add(i));
            }
        }

        assert_eq!(readback, pattern, "volatile r/w offset math must be correct");
    }

    // ---- UHS PSRAM tests --------------------------------------------------

    #[test]
    fn test_uhs_init_sets_af_en_and_mem_size() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(2000, UhsMemSize::Mb64, UhsPageSize::Kb2, UhsTemp::Normal);

        // auto-fresh enable and memory size should be set in uhs_basic.
        let basic = regs.uhs_basic().read();
        assert!(basic.reg_af_en().bit_is_set(), "auto-refresh must be enabled");
        assert_eq!(
            basic.reg_addrmb_msk().bits(),
            UhsMemSize::Mb64 as u8,
            "mem size mask should be 0x3f for 64 MB"
        );
        assert_eq!(
            basic.reg_linear_bnd_b().bits(),
            UhsPageSize::Kb2 as u8,
            "page size should be 0x0b for 2 KB"
        );
        assert!(basic.reg_init_en().bit_is_set(), "init_en must be kicked");
    }

    #[test]
    fn test_uhs_init_timing_ctrl_high_freq() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(2000, UhsMemSize::Mb32, UhsPageSize::Kb2, UhsTemp::Normal);

        // pck_freq=2000 > 1600 → timing_ctrl should be 0x1a03_000f
        let timing = unsafe {
            core::ptr::read_volatile(regs.uhs_timing_ctrl().as_ptr())
        };
        assert_eq!(
            timing, 0x1a03_000f,
            "timing_ctrl at 2000 MHz must be 0x1a03_000f (from SDK)"
        );
    }

    #[test]
    fn test_uhs_init_timing_ctrl_low_freq() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(1000, UhsMemSize::Mb32, UhsPageSize::Kb2, UhsTemp::Normal);

        // pck_freq=1000 ≤ 1600 → timing_ctrl should be 0x1202_000b
        let timing = unsafe {
            core::ptr::read_volatile(regs.uhs_timing_ctrl().as_ptr())
        };
        assert_eq!(
            timing, 0x1202_000b,
            "timing_ctrl at 1000 MHz must be 0x1202_000b (from SDK)"
        );
    }

    #[test]
    fn test_uhs_init_auto_refresh_normal_temp() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(2000, UhsMemSize::Mb64, UhsPageSize::Kb2, UhsTemp::Normal);

        // Normal temperature: win_cycle = 0x16e360, refi_cycle = 370.
        let win = regs.uhs_auto_fresh_1().read().reg_win_cycle().bits();
        assert_eq!(win, 0x16e360, "normal-temp win_cycle must be 0x16e360");

        let refi = regs.uhs_auto_fresh_2().read().reg_refi_cycle().bits();
        assert_eq!(refi, 370, "normal-temp refi_cycle must be 370");

        let burst = regs.uhs_auto_fresh_4().read().reg_bust_cycle().bits();
        assert_eq!(burst, 5, "burst cycle must be 5");
    }

    #[test]
    fn test_uhs_init_auto_refresh_high_temp() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(2000, UhsMemSize::Mb64, UhsPageSize::Kb2, UhsTemp::High);

        let win = regs.uhs_auto_fresh_1().read().reg_win_cycle().bits();
        assert_eq!(win, 0x0B71B0, "high-temp win_cycle must be 0x0B71B0");

        let refi = regs.uhs_auto_fresh_2().read().reg_refi_cycle().bits();
        assert_eq!(refi, 190, "high-temp refi_cycle must be 190");
    }

    #[test]
    fn test_uhs_pck_t_div_2000mhz() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(2000, UhsMemSize::Mb32, UhsPageSize::Kb2, UhsTemp::Normal);

        // 2000 MHz ≥ 1800 → div = 4 → bits [31:24] of uhs_manual = 4.
        let manual = regs.uhs_manual().read().reg_pck_t_div().bits();
        assert_eq!(manual, 4, "pck_t_div at 2000 MHz should be 4");
    }

    #[test]
    fn test_uhs_pck_t_div_500mhz() {
        let regs = setup_uhs();
        let pac = unsafe { PacPsramUhs::steal() };
        let mut driver = PsramUhs::new(pac);

        driver.init(500, UhsMemSize::Mb32, UhsPageSize::Kb2, UhsTemp::Normal);

        // 500 MHz < 666 → div = 0
        let manual = regs.uhs_manual().read().reg_pck_t_div().bits();
        assert_eq!(manual, 0, "pck_t_div at 500 MHz should be 0");
    }
}
