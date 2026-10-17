//! Electronic Fuse (EFUSE) Driver
//!
//! Uses real PAC registers from `bl808_pac::efuse`:
//!
//! - `trigger_read`       → EF_CTRL_IF_CTRL_0: ef_if_0_rw=0, ef_if_0_trig=1
//! - `trigger_program`    → EF_CTRL_IF_CTRL_0: ef_if_0_rw=1, ef_if_0_trig=1
//! - `is_busy`            → EF_CTRL_IF_CTRL_0: ef_if_0_busy (bit 2)
//! - `is_write_protected` → EF_DATA_0_LOCK: wr_lock_boot_mode (bit 14)
//!
//! The old stub used a single "bit 31" WP flag which has no direct PAC
//! equivalent. We expose the boot-mode write-lock bit as the representative
//! WP signal. Callers that need per-slot granularity should use EF_DATA_0_LOCK
//! / EF_DATA_1_LOCK fields directly.

#[cfg(feature = "m0")]
use bl808_pac::Efuse as PacEfuse;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_EFUSE_PTR: core::cell::Cell<*const bl808_pac::efuse::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(feature = "m0")]
pub struct Efuse {
    _inner: PacEfuse,
}

#[cfg(feature = "m0")]
impl Efuse {
    pub fn new(efuse: PacEfuse) -> Self {
        Self { _inner: efuse }
    }

    fn regs(&self) -> &bl808_pac::efuse::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacEfuse::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_EFUSE_PTR.with(|p| p.get()) };
    }

    /// Trigger an eFuse read operation.
    ///
    /// Sets ef_if_0_rw=0 (read) and pulses ef_if_0_trig=1 in EF_CTRL_IF_CTRL_0.
    /// Poll [`is_busy`] to wait for completion.
    pub fn trigger_read(&mut self) {
        self.regs().ef_ctrl_if_ctrl_0().modify(|_, w| {
            w.ef_if_0_rw().clear_bit(); // 0 = read
            w.ef_if_0_trig().set_bit()  // pulse trigger
        });
    }

    /// Returns `true` if the boot-mode region write-lock bit is set in EF_DATA_0_LOCK.
    ///
    /// This maps to `wr_lock_boot_mode` (bit 14 of EF_DATA_0_LOCK).
    /// For per-slot lock granularity access the lock register fields directly.
    pub fn is_write_protected(&self) -> bool {
        self.regs()
            .ef_data_0_lock()
            .read()
            .wr_lock_boot_mode()
            .bit_is_set()
    }

    /// Returns `true` if the eFuse interface 0 is still busy with a read/program operation.
    ///
    /// Maps to `ef_if_0_busy` (bit 2) in EF_CTRL_IF_CTRL_0.
    pub fn is_busy(&self) -> bool {
        self.regs()
            .ef_ctrl_if_ctrl_0()
            .read()
            .ef_if_0_busy()
            .bit_is_set()
    }

    /// Trigger an eFuse program (write) operation.
    ///
    /// Sets ef_if_0_rw=1 (program) and pulses ef_if_0_trig=1 in EF_CTRL_IF_CTRL_0.
    /// Poll [`is_busy`] to wait for completion.
    pub fn trigger_program(&mut self) {
        self.regs().ef_ctrl_if_ctrl_0().modify(|_, w| {
            w.ef_if_0_rw().set_bit();  // 1 = program/write
            w.ef_if_0_trig().set_bit() // pulse trigger
        });
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::efuse::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_EFUSE_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    fn make_driver() -> Efuse {
        let pac = unsafe { bl808_pac::Efuse::steal() };
        Efuse::new(pac)
    }

    /// trigger_read must clear ef_if_0_rw (bit 3) and set ef_if_0_trig (bit 4)
    /// in EF_CTRL_IF_CTRL_0.
    #[test]
    fn test_efuse_read_trigger() {
        let regs = setup();
        let mut driver = make_driver();

        driver.trigger_read();

        let v = regs.ef_ctrl_if_ctrl_0().read();
        assert!(
            v.ef_if_0_trig().bit_is_set(),
            "ef_if_0_trig (bit 4) must be set after trigger_read"
        );
        assert!(
            !v.ef_if_0_rw().bit_is_set(),
            "ef_if_0_rw (bit 3) must be 0 for a read operation"
        );
    }

    /// trigger_program must set ef_if_0_rw (bit 3) and ef_if_0_trig (bit 4).
    #[test]
    fn test_efuse_program_trigger() {
        let regs = setup();
        let mut driver = make_driver();

        driver.trigger_program();

        let v = regs.ef_ctrl_if_ctrl_0().read();
        assert!(
            v.ef_if_0_trig().bit_is_set(),
            "ef_if_0_trig (bit 4) must be set after trigger_program"
        );
        assert!(
            v.ef_if_0_rw().bit_is_set(),
            "ef_if_0_rw (bit 3) must be 1 for a program operation"
        );
    }

    /// is_write_protected reads wr_lock_boot_mode (bit 14) from EF_DATA_0_LOCK.
    #[test]
    fn test_efuse_write_protect_check() {
        let regs = setup();
        let driver = make_driver();

        // Poke wr_lock_boot_mode (bit 14) into EF_DATA_0_LOCK via raw pointer.
        unsafe {
            core::ptr::write_volatile(regs.ef_data_0_lock().as_ptr(), 1u32 << 14);
        }
        assert!(driver.is_write_protected(), "should be write-protected when bit 14 is set");

        unsafe {
            core::ptr::write_volatile(regs.ef_data_0_lock().as_ptr(), 0u32);
        }
        assert!(!driver.is_write_protected(), "should not be write-protected when bit 14 is clear");
    }

    /// is_busy reads ef_if_0_busy (bit 2) from EF_CTRL_IF_CTRL_0.
    #[test]
    fn test_efuse_busy_poll() {
        let regs = setup();
        let driver = make_driver();

        // Set ef_if_0_busy (bit 2) directly.
        unsafe {
            core::ptr::write_volatile(regs.ef_ctrl_if_ctrl_0().as_ptr(), 1u32 << 2);
        }
        assert!(driver.is_busy(), "should report busy when bit 2 is set");

        unsafe {
            core::ptr::write_volatile(regs.ef_ctrl_if_ctrl_0().as_ptr(), 0u32);
        }
        assert!(!driver.is_busy(), "should not report busy when bit 2 is clear");
    }
}
