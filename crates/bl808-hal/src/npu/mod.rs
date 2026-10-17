//! Neural Processing Unit (NPU / BLAI) Driver
//!
//! Provides PAC-backed access to the BL808 NPU (BLAI) engine.
//!
//! Register offsets (from the d0 PAC):
//!   0x00 - bali_general_cfg
//!   0x04 - blai_int_cfg  (start/stop/int_clr write-1-pulse; sts_blai_int read)
//!   0x18 - blai_inst_addr

use bl808_pac::Npu as PacNpu;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_NPU_PTR: core::cell::Cell<*const bl808_pac::npu::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

pub struct Npu {
    _inner: PacNpu,
}

impl Npu {
    pub fn new(npu: PacNpu) -> Self {
        Self { _inner: npu }
    }

    fn regs(&self) -> &bl808_pac::npu::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacNpu::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_NPU_PTR.with(|p| p.get()) };
    }

    /// Load the instruction buffer base address and start NPU execution.
    ///
    /// # Safety
    /// `inst_addr` must point to a valid, DMA-accessible instruction buffer that
    /// remains live until [`poll_done`] returns `true`.
    pub unsafe fn start(&mut self, inst_addr: u32) {
        let regs = self.regs();
        regs.blai_inst_addr()
            .write(|w| w.reg_inst_addr_base().bits(inst_addr));
        // Write-1-pulse: write only the start bit; do not read-modify-write.
        regs.blai_int_cfg()
            .write(|w| w.reg_blai_start().set_bit());
    }

    /// Stop NPU execution immediately.
    pub fn stop(&mut self) {
        self.regs()
            .blai_int_cfg()
            .write(|w| w.reg_blai_stop().set_bit());
    }

    /// Returns `true` when the NPU has signalled task completion.
    pub fn poll_done(&self) -> bool {
        self.regs().blai_int_cfg().read().sts_blai_int().bit_is_set()
    }

    /// Clear the done interrupt flag.
    pub fn clear_interrupt(&mut self) {
        self.regs()
            .blai_int_cfg()
            .write(|w| w.reg_int_clr().set_bit());
    }

    /// Blocking execute: load instructions and spin until done, then clear the interrupt.
    ///
    /// Returns `true` if the NPU signalled completion within the spin budget,
    /// `false` if the budget was exhausted (hardware did not signal done in time).
    ///
    /// The budget of `10_000_000` iterations is generous enough for typical NPU
    /// inference workloads; a stuck device will no longer hang the CPU forever.
    ///
    /// For non-blocking use, call [`start`] then poll [`poll_done`] directly and
    /// call [`clear_interrupt`] yourself when it returns `true`.
    ///
    /// # Safety
    /// Same contract as [`start`].
    pub unsafe fn execute_blocking(&mut self, inst_addr: u32) -> bool {
        const SPIN_BUDGET: u32 = 10_000_000;
        self.start(inst_addr);
        for _ in 0..SPIN_BUDGET {
            if self.poll_done() {
                self.clear_interrupt();
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }
}

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use bl808_pac::npu::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_NPU_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_npu_start_writes_inst_addr_and_start_bit() {
        let regs = setup();
        let npu = unsafe { PacNpu::steal() };
        let mut driver = Npu::new(npu);

        unsafe { driver.start(0xDEAD_BEEFu32) };

        // blai_inst_addr is write-only; read back via as_ptr()
        let inst_val = unsafe { core::ptr::read_volatile(regs.blai_inst_addr().as_ptr()) };
        assert_eq!(inst_val, 0xDEAD_BEEF, "inst_addr should be written");

        // blai_int_cfg: reg_blai_start is bit 0
        let int_cfg = regs.blai_int_cfg().read();
        // After start() writes a 1-pulse and then start() returns, bit 0 was written.
        // We read it back raw since sts_blai_start isn't a reader field — use as_ptr().
        let int_raw = unsafe { core::ptr::read_volatile(regs.blai_int_cfg().as_ptr()) };
        assert_ne!(int_raw & 1, 0, "reg_blai_start pulse should be written");
        let _ = int_cfg;
    }

    #[test]
    fn test_npu_stop_writes_stop_bit() {
        let regs = setup();
        let npu = unsafe { PacNpu::steal() };
        let mut driver = Npu::new(npu);

        driver.stop();

        // reg_blai_stop is bit 1
        let int_raw = unsafe { core::ptr::read_volatile(regs.blai_int_cfg().as_ptr()) };
        assert_ne!(int_raw & (1 << 1), 0, "reg_blai_stop pulse should be written");
    }

    #[test]
    fn test_npu_poll_done_reads_sts_bit() {
        let regs = setup();
        let npu = unsafe { PacNpu::steal() };
        let driver = Npu::new(npu);

        // sts_blai_int is bit 9 of blai_int_cfg
        assert!(!driver.poll_done(), "should be not done initially");

        unsafe {
            core::ptr::write_volatile(regs.blai_int_cfg().as_ptr(), 1 << 9);
        }
        assert!(driver.poll_done(), "should be done after bit 9 is set");
    }

    #[test]
    fn test_npu_clear_interrupt_writes_int_clr_bit() {
        let regs = setup();
        let npu = unsafe { PacNpu::steal() };
        let mut driver = Npu::new(npu);

        driver.clear_interrupt();

        // reg_int_clr is bit 8
        let int_raw = unsafe { core::ptr::read_volatile(regs.blai_int_cfg().as_ptr()) };
        assert_ne!(int_raw & (1 << 8), 0, "reg_int_clr pulse should be written");
    }

    #[test]
    fn test_npu_execute_blocking_signature_returns_bool() {
        // Compile-time / API-contract check: confirm execute_blocking returns bool.
        // The individual building blocks (start, poll_done, clear_interrupt) are each
        // tested above. The timeout path would require 10M iterations; the success path
        // cannot be exercised here because start() PAC-writes blai_int_cfg (overwriting
        // any pre-injected sts_blai_int bit) before the poll loop runs.
        let _ : unsafe fn(&mut Npu, u32) -> bool = Npu::execute_blocking;
    }
}
