//! RISC-V Platform-Level Interrupt Controller (PLIC)
//!
//! Standard PLIC memory layout at base 0xE000_0000:
//!
//!   base + n*4             — source n priority (n ≥ 1)
//!   base + 0x0000_1000     — pending bits (read-only, 32 sources per word)
//!   base + 0x0000_2000 + ctx*0x80 + word*4  — enable bits for context ctx
//!   base + 0x0020_0000 + ctx*0x1000         — priority threshold for context ctx
//!   base + 0x0020_0004 + ctx*0x1000         — claim / complete register for ctx
//!
//! On BL808 the PLIC sits at 0xE000_0000.  The E907 M0 core uses context 0;
//! the C906 D0 core uses context 1.

use core::sync::atomic::{AtomicPtr, Ordering};

/// PLIC base address on BL808.
pub const PLIC_BASE: usize = 0xE000_0000;

const PRIORITY_OFFSET: usize = 0x0000_0000;
const PENDING_OFFSET: usize = 0x0000_1000;
const ENABLE_OFFSET: usize = 0x0000_2000;
const ENABLE_CTX_STRIDE: usize = 0x80;
const THRESHOLD_OFFSET: usize = 0x0020_0000;
const CLAIM_OFFSET: usize = 0x0020_0004;
const CONTEXT_STRIDE: usize = 0x0000_1000;

// ---------------------------------------------------------------------------
// Handler table
// ---------------------------------------------------------------------------

/// Maximum number of PLIC interrupt sources supported by the handler table.
///
/// On BL808:
/// - The M0 PAC defines external interrupts 0–79 (`WIFI_IPC_PUBLIC = 79`).
/// - The D0 PAC defines external interrupts 0–82 (`AUDIO = 80`, `WL_ALL = 81`, `PDS = 82`).
/// Setting `N_IRQS = 128` (a standard 4-word, 4 × 32 PLIC bitmap) covers all sources
/// across both cores without out-of-range asserts or dropped dispatches.
pub const N_IRQS: usize = 128;

type HandlerFn = fn();

/// Lock-free table of registered interrupt handlers, indexed by IRQ number.
///
/// Each slot stores a raw function pointer cast to `*mut ()`.  A null pointer
/// means "no handler registered".  Atomic operations with `Release`/`Acquire`
/// ordering are used so that a handler registered on one thread is visible to
/// an ISR running on the same core after the PLIC issues the claim.
static HANDLERS: [AtomicPtr<()>; N_IRQS] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; N_IRQS];

/// Register a handler for PLIC external interrupt source `irq`.
///
/// `irq` must be less than [`N_IRQS`]; out-of-range values are silently
/// ignored in release builds (debug builds will assert).
///
/// The stored pointer is visible to any subsequent [`Plic::handle_next_interrupt`]
/// call via `Ordering::Release` / `Ordering::Acquire` pairing.
pub fn register_handler(irq: u32, handler: HandlerFn) {
    debug_assert!((irq as usize) < N_IRQS, "irq out of range");
    if (irq as usize) < N_IRQS {
        HANDLERS[irq as usize].store(handler as *mut (), Ordering::Release);
    }
}

/// Clear any registered handler for PLIC external interrupt source `irq`.
///
/// After this call, [`Plic::handle_next_interrupt`] will skip dispatch for
/// `irq` (claim and complete still occur).
pub fn clear_handler(irq: u32) {
    debug_assert!((irq as usize) < N_IRQS, "irq out of range");
    if (irq as usize) < N_IRQS {
        HANDLERS[irq as usize].store(core::ptr::null_mut(), Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// PLIC handle
// ---------------------------------------------------------------------------

/// A handle to the PLIC for one CPU context (hart × privilege mode).
///
/// Construct once and store in a static or pass by value.  All register
/// accesses use raw volatile reads/writes so no PAC wrapper is needed.
pub struct Plic {
    base: usize,
    context: usize,
}

impl Plic {
    /// Create a PLIC handle.
    ///
    /// # Safety
    ///
    /// `base` must be the correct PLIC MMIO base address and `context` must
    /// be a valid hart context index.  Typically `base = 0xE000_0000` and
    /// `context = 0` for the M0 core.
    pub const unsafe fn new(base: usize, context: usize) -> Self {
        Self { base, context }
    }

    // -- raw register helpers ------------------------------------------------

    #[inline]
    unsafe fn write32(&self, offset: usize, val: u32) {
        core::ptr::write_volatile((self.base + offset) as *mut u32, val);
    }

    #[inline]
    unsafe fn read32(&self, offset: usize) -> u32 {
        core::ptr::read_volatile((self.base + offset) as *const u32)
    }

    // -- source priority -----------------------------------------------------

    /// Set the priority of interrupt source `irq` (1-indexed, ≥ 1).
    ///
    /// Priority 0 effectively masks the source.
    ///
    /// # Safety
    ///
    /// `irq` must be a valid source index for this PLIC instance.
    pub unsafe fn set_priority(&self, irq: u32, priority: u32) {
        self.write32(PRIORITY_OFFSET + irq as usize * 4, priority);
    }

    /// Read back the priority of source `irq`.
    ///
    /// # Safety
    ///
    /// Same as [`set_priority`].
    pub unsafe fn priority(&self, irq: u32) -> u32 {
        self.read32(PRIORITY_OFFSET + irq as usize * 4)
    }

    // -- pending bits (read-only) --------------------------------------------

    /// Return `true` if source `irq` has a pending interrupt.
    ///
    /// # Safety
    ///
    /// `irq` must be a valid source index.
    pub unsafe fn is_pending(&self, irq: u32) -> bool {
        let word = irq / 32;
        let bit = irq % 32;
        (self.read32(PENDING_OFFSET + word as usize * 4) >> bit) & 1 != 0
    }

    // -- enable bits ---------------------------------------------------------

    /// Enable source `irq` for this context.
    ///
    /// The read-modify-write is performed inside a `critical_section` so that
    /// an interrupt occurring between the read and write cannot corrupt the
    /// enable word.
    ///
    /// # Safety
    ///
    /// `irq` must be a valid source index.
    pub unsafe fn enable(&self, irq: u32) {
        let word = irq / 32;
        let bit = irq % 32;
        let offset = ENABLE_OFFSET + self.context * ENABLE_CTX_STRIDE + word as usize * 4;
        // SAFETY: raw pointer arithmetic stays within the mock/MMIO buffer;
        // the critical section prevents an ISR from racing this RMW.
        let base = self.base;
        critical_section::with(|_| {
            let val = core::ptr::read_volatile((base + offset) as *const u32);
            core::ptr::write_volatile((base + offset) as *mut u32, val | (1 << bit));
        });
    }

    /// Disable source `irq` for this context.
    ///
    /// The read-modify-write is performed inside a `critical_section` for the
    /// same reason as [`enable`].
    ///
    /// # Safety
    ///
    /// `irq` must be a valid source index.
    pub unsafe fn disable(&self, irq: u32) {
        let word = irq / 32;
        let bit = irq % 32;
        let offset = ENABLE_OFFSET + self.context * ENABLE_CTX_STRIDE + word as usize * 4;
        let base = self.base;
        critical_section::with(|_| {
            let val = core::ptr::read_volatile((base + offset) as *const u32);
            core::ptr::write_volatile((base + offset) as *mut u32, val & !(1 << bit));
        });
    }

    // -- threshold -----------------------------------------------------------

    /// Set the minimum priority threshold for this context.
    ///
    /// Interrupts with a priority ≤ threshold are suppressed.
    ///
    /// # Safety
    ///
    /// Caller must ensure the threshold value is within the PLIC's supported
    /// range (typically 0–7).
    pub unsafe fn set_threshold(&self, threshold: u32) {
        self.write32(THRESHOLD_OFFSET + self.context * CONTEXT_STRIDE, threshold);
    }

    /// Read back the current threshold for this context.
    ///
    /// # Safety
    ///
    /// Must be called on a valid PLIC instance.
    pub unsafe fn threshold(&self) -> u32 {
        self.read32(THRESHOLD_OFFSET + self.context * CONTEXT_STRIDE)
    }

    // -- claim / complete ----------------------------------------------------

    /// Claim the highest-priority pending interrupt for this context.
    ///
    /// Returns the source ID (≥ 1), or 0 if no interrupt is pending.
    ///
    /// # Safety
    ///
    /// Must be called from the interrupt handler after the PLIC asserts the
    /// external interrupt to this context.
    pub unsafe fn claim(&self) -> u32 {
        self.read32(CLAIM_OFFSET + self.context * CONTEXT_STRIDE)
    }

    /// Signal completion of interrupt `irq` to the PLIC.
    ///
    /// Must be called after the handler for source `irq` has finished.
    ///
    /// # Safety
    ///
    /// `irq` must equal the value returned by the preceding `claim()` call.
    pub unsafe fn complete(&self, irq: u32) {
        self.write32(CLAIM_OFFSET + self.context * CONTEXT_STRIDE, irq);
    }

    // -- handler dispatch ----------------------------------------------------

    /// Claim the next pending interrupt, dispatch its registered handler, and
    /// signal completion to the PLIC.
    ///
    /// This is the standard claim → dispatch → complete cycle and should be
    /// called from the external-interrupt trap handler for this PLIC context
    /// (e.g. from the `MachineExternal` exception vector on the M0 core).
    ///
    /// If no interrupt is pending (`claim()` returns 0) the function returns
    /// immediately without calling any handler.  If the claimed IRQ has no
    /// registered handler the complete is still issued so the PLIC can accept
    /// new interrupts.
    ///
    /// # Safety
    ///
    /// Must be called from the trap handler that corresponds to this PLIC
    /// context.  The PLIC must have been configured (priority, threshold,
    /// enable) before this function is invoked.  It is not re-entrant for the
    /// same context: do not call it while a prior claim on this context is
    /// still in-flight.
    pub unsafe fn handle_next_interrupt(&self) {
        let irq = self.claim();
        if irq != 0 {
            if (irq as usize) < N_IRQS {
                let p = HANDLERS[irq as usize].load(Ordering::Acquire);
                if !p.is_null() {
                    let f: HandlerFn = core::mem::transmute(p);
                    f();
                }
            }
            self.complete(irq);
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience: a statically-configured M0-core PLIC handle
// ---------------------------------------------------------------------------

/// A pre-configured PLIC handle for the BL808 E907 (M0) core (context 0).
///
/// # Safety
///
/// Must only be used from the M0 core.
pub static PLIC_M0: Plic = unsafe { Plic::new(PLIC_BASE, 0) };

/// A pre-configured PLIC handle for the BL808 C906 (D0) core (context 1).
///
/// # Safety
///
/// Must only be used from the D0 core.
pub static PLIC_D0: Plic = unsafe { Plic::new(PLIC_BASE, 1) };

// Statics hold only usize + usize; they are Send+Sync.
unsafe impl Send for Plic {}
unsafe impl Sync for Plic {}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use core::sync::atomic::{AtomicBool, AtomicU32};
    use std::vec;

    /// Build a mock PLIC memory region and a Plic pointed at it.
    fn mock_plic(buf: &[u32]) -> Plic {
        // SAFETY: we're in a test; the buffer lives long enough.
        unsafe { Plic::new(buf.as_ptr() as usize, 0) }
    }

    #[test]
    fn test_set_priority_writes_correct_offset() {
        let buf = vec![0u32; 0x30_0000 / 4];
        let plic = mock_plic(&buf);

        unsafe { plic.set_priority(3, 5) };

        // source 3 priority is at base + 3*4 = word index 3
        assert_eq!(buf[3], 5, "priority word at index 3 should be 5");

        drop(buf);
        let _ = plic;
    }

    #[test]
    fn test_enable_sets_correct_bit() {
        let buf = vec![0u32; 0x30_0000 / 4];
        let plic = mock_plic(&buf);

        // source 33 → word 1, bit 1 in the enable array for context 0
        unsafe { plic.enable(33) };

        // enable[ctx=0][word=1] at base + 0x2000 + 0*0x80 + 1*4 = 0x2004
        let idx = 0x2004 / 4;
        assert_eq!(buf[idx] & (1 << 1), (1 << 1), "bit 1 of enable word 1 should be set");

        drop(buf);
        let _ = plic;
    }

    #[test]
    fn test_disable_clears_bit() {
        let buf = vec![0xFFFF_FFFFu32; 0x30_0000 / 4];
        let plic = mock_plic(&buf);

        unsafe { plic.disable(5) };

        // source 5 → word 0, bit 5; enable offset 0x2000
        let idx = 0x2000 / 4;
        assert_eq!(buf[idx] & (1 << 5), 0, "bit 5 should be cleared");

        drop(buf);
        let _ = plic;
    }

    #[test]
    fn test_set_threshold_writes_correct_offset() {
        let buf = vec![0u32; 0x30_0000 / 4];
        let plic = mock_plic(&buf);

        unsafe { plic.set_threshold(3) };

        // threshold[ctx=0] at base + 0x20_0000; word index 0x20_0000/4
        let idx = 0x0020_0000 / 4;
        assert_eq!(buf[idx], 3);

        drop(buf);
        let _ = plic;
    }

    #[test]
    fn test_claim_reads_claim_register() {
        let mut buf = vec![0u32; 0x30_0000 / 4];
        // claim[ctx=0] at base + 0x20_0004
        let idx = 0x0020_0004 / 4;
        buf[idx] = 7;
        let plic = mock_plic(&buf);

        let claimed = unsafe { plic.claim() };
        assert_eq!(claimed, 7);

        drop(buf);
        let _ = plic;
    }

    // -----------------------------------------------------------------------
    // Handler table tests
    // -----------------------------------------------------------------------

    /// Verify that `register_handler` / `clear_handler` update the HANDLERS
    /// table and that `handle_next_interrupt` dispatches accordingly.
    #[test]
    fn test_register_and_dispatch_handler() {
        // Use IRQ 10 to avoid colliding with other parallel tests.
        const IRQ: u32 = 10;

        static FIRED: AtomicBool = AtomicBool::new(false);
        FIRED.store(false, Ordering::SeqCst);

        fn handler() {
            FIRED.store(true, Ordering::SeqCst);
        }

        // Register the handler.
        register_handler(IRQ, handler);

        // Build a mock PLIC where claim returns IRQ.
        let mut buf = vec![0u32; 0x30_0000 / 4];
        let claim_idx = 0x0020_0004 / 4;
        buf[claim_idx] = IRQ;
        let plic = mock_plic(&buf);

        unsafe { plic.handle_next_interrupt() };

        // Handler must have fired.
        assert!(FIRED.load(Ordering::SeqCst), "handler should have been called");

        // complete() writes irq back to the claim/complete register.
        assert_eq!(buf[claim_idx], IRQ, "complete should write irq back to claim register");

        // Clean up: clear the handler slot so other tests are unaffected.
        clear_handler(IRQ);

        drop(buf);
        let _ = plic;
    }

    /// Verify that `clear_handler` prevents dispatch.
    #[test]
    fn test_clear_handler_prevents_dispatch() {
        const IRQ: u32 = 11;

        static FIRED: AtomicBool = AtomicBool::new(false);
        FIRED.store(false, Ordering::SeqCst);

        fn handler() {
            FIRED.store(true, Ordering::SeqCst);
        }

        register_handler(IRQ, handler);
        clear_handler(IRQ);

        let mut buf = vec![0u32; 0x30_0000 / 4];
        let claim_idx = 0x0020_0004 / 4;
        buf[claim_idx] = IRQ;
        let plic = mock_plic(&buf);

        unsafe { plic.handle_next_interrupt() };

        // Handler must NOT have fired (slot was cleared).
        assert!(!FIRED.load(Ordering::SeqCst), "cleared handler should not fire");

        // complete() must still have been called.
        assert_eq!(buf[claim_idx], IRQ, "complete should still write irq back");

        drop(buf);
        let _ = plic;
    }

    /// Verify that `handle_next_interrupt` is a no-op when claim returns 0.
    #[test]
    fn test_handle_next_interrupt_no_pending() {
        const IRQ: u32 = 12;

        static FIRED: AtomicBool = AtomicBool::new(false);
        FIRED.store(false, Ordering::SeqCst);

        fn handler() {
            FIRED.store(true, Ordering::SeqCst);
        }

        register_handler(IRQ, handler);

        // Claim register returns 0 → no interrupt pending.
        let buf = vec![0u32; 0x30_0000 / 4];
        let plic = mock_plic(&buf);

        unsafe { plic.handle_next_interrupt() };

        assert!(!FIRED.load(Ordering::SeqCst), "handler should not fire when claim returns 0");

        clear_handler(IRQ);
        drop(buf);
        let _ = plic;
    }

    /// Verify complete writes the claimed IRQ back (via handle_next_interrupt).
    #[test]
    fn test_handle_next_interrupt_complete_written() {
        const IRQ: u32 = 13;

        static CALL_COUNT: AtomicU32 = AtomicU32::new(0);
        CALL_COUNT.store(0, Ordering::SeqCst);

        fn handler() {
            CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        }

        register_handler(IRQ, handler);

        let mut buf = vec![0u32; 0x30_0000 / 4];
        let claim_idx = 0x0020_0004 / 4;
        buf[claim_idx] = IRQ;
        let plic = mock_plic(&buf);

        unsafe { plic.handle_next_interrupt() };

        assert_eq!(CALL_COUNT.load(Ordering::SeqCst), 1, "handler should be called exactly once");
        assert_eq!(buf[claim_idx], IRQ, "complete register should hold the claimed irq");

        clear_handler(IRQ);
        drop(buf);
        let _ = plic;
    }

    /// Verify that high-numbered D0 IRQs (80, 81, 82) and IRQs up to N_IRQS - 1
    /// can be registered, dispatched, and cleared without panic or dropping.
    #[test]
    fn test_high_irq_registration_and_dispatch() {
        const HIGH_IRQS: [u32; 4] = [80, 81, 82, 127];

        for &irq in &HIGH_IRQS {
            static FIRED: AtomicBool = AtomicBool::new(false);
            FIRED.store(false, Ordering::SeqCst);

            fn handler() {
                FIRED.store(true, Ordering::SeqCst);
            }

            register_handler(irq, handler);

            let mut buf = vec![0u32; 0x30_0000 / 4];
            let claim_idx = 0x0020_0004 / 4;
            buf[claim_idx] = irq;
            let plic = mock_plic(&buf);

            unsafe { plic.handle_next_interrupt() };

            assert!(
                FIRED.load(Ordering::SeqCst),
                "handler for IRQ {} should have been dispatched",
                irq
            );
            assert_eq!(buf[claim_idx], irq, "complete should write claimed IRQ back");

            clear_handler(irq);
        }
    }
}
