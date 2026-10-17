//! Power Domain Sleep (PDS) Driver for BL808
//!
//! PDS controls SoC-level sleep/wake-up and power-rail management.
//! This driver provides access to the real PAC registers for:
//! - Entering power-save mode (`enter_sleep`)
//! - Configuring wakeup sources via `PDS_INT.cr_pds_wakeup_src_en`
//! - Reading PDS FSM state from `PDS_STAT.ro_pds_state`
//! - Configuring GPIO wakeup edge via `PDS_GPIO_INT`
//! - Clearing the reset-event latch

use bl808_pac::Pds as PacPds;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_PDS_PTR: core::cell::Cell<*const bl808_pac::pds::RegisterBlock>
        = const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// Wakeup source bits (PDS_INT.cr_pds_wakeup_src_en, bits 10:20)
// ---------------------------------------------------------------------------

/// Wakeup source enable flags for [`Pds::set_wakeup_sources`].
#[derive(Clone, Copy, Debug, Default)]
pub struct WakeupSources(pub u16);

impl WakeupSources {
    pub const NONE: Self = Self(0);
    pub const GPIO: Self = Self(1 << 0);
    pub const RF_DONE: Self = Self(1 << 1);
    pub const TBTT_SLEEP: Self = Self(1 << 2);
    pub const TBTT_WAKEUP: Self = Self(1 << 3);
    pub const ALL: Self = Self(0x7ff);

    pub fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

// ---------------------------------------------------------------------------
// GPIO wakeup edge mode (PDS_GPIO_INT fields)
// ---------------------------------------------------------------------------

/// GPIO wakeup edge/level mode (4-bit field per GPIO set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GpioWakeupMode {
    SyncFallingEdge = 0,
    SyncRisingEdge = 1,
    SyncLowLevel = 2,
    SyncHighLevel = 3,
    AsyncFallingEdge = 4,
    AsyncRisingEdge = 5,
}

/// Which GPIO wakeup set to configure (BL808 has 4 sets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioSet {
    Set1,
    Set2,
    Set3,
    Set4,
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

pub struct Pds {
    _inner: PacPds,
}

impl Pds {
    pub fn new(pds: PacPds) -> Self {
        Self { _inner: pds }
    }

    fn regs(&self) -> &bl808_pac::pds::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacPds::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_PDS_PTR.with(|p| p.get()) };
    }

    // -----------------------------------------------------------------------
    // Wakeup source configuration
    // -----------------------------------------------------------------------

    /// Enable the specified wakeup sources and clear any pending interrupt.
    pub fn set_wakeup_sources(&mut self, sources: WakeupSources) {
        self.regs().pds_int().modify(|_, w| unsafe {
            w.cr_pds_wakeup_src_en()
                .bits(sources.0)
                .cr_pds_int_clr()
                .set_bit()
        });
    }

    /// Read the raw wakeup event flags (which sources fired on last wakeup).
    pub fn wakeup_event(&self) -> u16 {
        self.regs().pds_int().read().ro_pds_wakeup_event().bits()
    }

    // -----------------------------------------------------------------------
    // PDS FSM state
    // -----------------------------------------------------------------------

    /// Read the PDS finite-state-machine state register.
    ///
    /// Returns `ro_pds_state` (5-bit field, 0 = IDLE, 1+ = active sleep steps).
    pub fn state(&self) -> u8 {
        self.regs().pds_stat().read().ro_pds_state().bits()
    }

    /// Read the reset-event flags from `PDS_STAT.pds_reset_event`.
    pub fn reset_event(&self) -> u8 {
        self.regs().pds_stat().read().pds_reset_event().bits()
    }

    /// Clear the reset-event latch (W1C).
    pub fn clear_reset_event(&mut self) {
        self.regs()
            .pds_stat()
            .write(|w| w.pds_clr_reset_event().clear_bit_by_one());
    }

    // -----------------------------------------------------------------------
    // GPIO wakeup
    // -----------------------------------------------------------------------

    /// Configure the wakeup edge mode for one of the four GPIO sets and clear
    /// any previous GPIO wakeup interrupt for that set.
    pub fn set_gpio_wakeup(&mut self, set: GpioSet, mode: GpioWakeupMode) {
        let m = mode as u8;
        self.regs().pds_gpio_int().modify(|_, w| unsafe {
            match set {
                GpioSet::Set1 => w
                    .pds_gpio_set1_int_mode()
                    .bits(m)
                    .pds_gpio_set1_int_clr()
                    .set_bit(),
                GpioSet::Set2 => w
                    .pds_gpio_set2_int_mode()
                    .bits(m)
                    .pds_gpio_set2_int_clr()
                    .set_bit(),
                GpioSet::Set3 => w
                    .pds_gpio_set3_int_mode()
                    .bits(m)
                    .pds_gpio_set3_int_clr()
                    .set_bit(),
                GpioSet::Set4 => w
                    .pds_gpio_set4_int_mode()
                    .bits(m)
                    .pds_gpio_set4_int_clr()
                    .set_bit(),
            }
        });
    }

    // -----------------------------------------------------------------------
    // Sleep entry
    // -----------------------------------------------------------------------

    /// Enter PDS power-save mode.
    ///
    /// - `sleep_forever`: if true, the SoC does not wake up automatically.
    /// - `gate_clk`: gate the system clock during sleep.
    /// - `mem_standby`: put RAM into standby during sleep.
    ///
    /// Callers must set up wakeup sources with [`set_wakeup_sources`] first.
    ///
    /// # Safety
    /// Calling this puts the SoC into a deep-sleep state.  Ensure all
    /// peripherals are idle and caches/TLBs are flushed before calling.
    pub unsafe fn enter_sleep(&mut self, sleep_forever: bool, gate_clk: bool, mem_standby: bool) {
        self.regs().pds_ctl().modify(|_, w| {
            w.cr_sleep_forever()
                .bit(sleep_forever)
                .cr_pds_gate_clk()
                .bit(gate_clk)
                .cr_pds_mem_stby()
                .bit(mem_standby)
                .cr_pds_iso_en()
                .set_bit()
                .cr_pds_pwr_off()
                .set_bit()
        });
        // Writing pds_start_ps triggers the sleep sequence.
        self.regs()
            .pds_ctl()
            .modify(|_, w| w.pds_start_ps().set_bit());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::pds::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_PDS_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_pds_wakeup_sources_written() {
        let regs = setup();
        let mut pds = Pds::new(unsafe { PacPds::steal() });

        let sources = WakeupSources::GPIO.or(WakeupSources::RF_DONE);
        pds.set_wakeup_sources(sources);

        assert_eq!(
            regs.pds_int().read().cr_pds_wakeup_src_en().bits(),
            sources.0,
            "Wakeup source enable bits should be written"
        );
    }

    #[test]
    fn test_pds_state_read() {
        let regs = setup();
        // ro_pds_state lives in the read-only PDS_STAT register; poke the mock
        // directly via the register's own address.
        unsafe { core::ptr::write_volatile(regs.pds_stat().as_ptr(), 0x05) };

        let pds = Pds::new(unsafe { PacPds::steal() });
        assert_eq!(pds.state(), 5, "PDS state should read 5");
    }

    #[test]
    fn test_pds_gpio_wakeup_mode() {
        let regs = setup();
        let mut pds = Pds::new(unsafe { PacPds::steal() });

        pds.set_gpio_wakeup(GpioSet::Set1, GpioWakeupMode::SyncRisingEdge);

        assert_eq!(
            regs.pds_gpio_int().read().pds_gpio_set1_int_mode().bits(),
            GpioWakeupMode::SyncRisingEdge as u8,
            "GPIO set1 wakeup mode should be SyncRisingEdge (1)"
        );
    }
}
