//! Inter-Processor Communication (IPC) Driver for BL808
//!
//! The BL808 IPC block provides 16 independent interrupt channels between the
//! M0 (CPU0 / E907), D0 (CPU1 / C906), and LP (E902) cores.
//!
//! # Channel semantics
//! Each channel is a single bit (0–15) in the 16-bit channel mask:
//! - **Trigger D0 (CPU1)**: write channel bit to `CPU1_IPC_ISWR` → D0 sees interrupt.
//! - **Read D0 pending**: read `CPU1_IPC_IRSRR` → which channels have fired on D0.
//! - **Clear D0 interrupt**: write channel bit to `CPU1_IPC_ICR` (W1C).
//! - **Unmask D0 channels**: write channel bits to `CPU1_IPC_IUSR`.
//!
//! - **Trigger M0 (CPU0)**: write channel bit to `CPU0_IPC_ISWR` → M0 sees interrupt.
//! - **Read M0 pending**: read `CPU0_IPC_IRSRR` → which channels have fired on M0.
//! - **Clear M0 interrupt**: write channel bit to `CPU0_IPC_ICR` (W1C).
//! - **Unmask M0 channels**: write channel bits to `CPU0_IPC_IUSR`.
//!
//! # Shared memory / mailbox
//! The IPC block only signals interrupts; payload data must be passed through
//! a shared OCRAM or PSRAM region agreed upon at link time. Place a
//! `#[link_section = ".shared"]` buffer in both core images and pass its
//! address via the linker map.

use bl808_pac::Ipc as PacIpc;

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_IPC_PTR: core::cell::Cell<*const bl808_pac::ipc::RegisterBlock>
        = const { core::cell::Cell::new(core::ptr::null()) };
}

/// Bitmask of IPC channels (bits 0–15).
pub type ChannelMask = u16;

/// IPC driver for the BL808 SoC.
///
/// Provides bi-directional signalling between the M0, D0, and LP cores.
pub struct Ipc {
    _inner: PacIpc,
}

impl Ipc {
    pub fn new(ipc: PacIpc) -> Self {
        Self { _inner: ipc }
    }

    fn regs(&self) -> &bl808_pac::ipc::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return unsafe { &*PacIpc::PTR };
        #[cfg(any(test, feature = "mock"))]
        return unsafe { &*MOCK_IPC_PTR.with(|p| p.get()) };
    }

    // -----------------------------------------------------------------------
    // Signalling toward D0 (CPU1)
    // -----------------------------------------------------------------------

    /// Raise an interrupt on D0 (CPU1) for the given channel mask.
    ///
    /// Writing 1 to a bit in `CPU1_IPC_ISWR` sets the corresponding pending
    /// bit in D0's interrupt-status register.
    pub fn trigger_d0(&mut self, channels: ChannelMask) {
        self.regs()
            .cpu1_ipc_iswr()
            .write(|w| unsafe { w.cpu1_ipc_iswr().bits(channels) });
    }

    /// Read the raw interrupt-pending status for D0 (CPU1).
    ///
    /// Each set bit indicates that M0 has signalled that channel to D0.
    /// Call [`clear_d0`] to acknowledge.
    pub fn pending_d0(&self) -> ChannelMask {
        self.regs()
            .cpu1_ipc_irsrr()
            .read()
            .bits() as ChannelMask
    }

    /// Clear the pending interrupt(s) on D0 (CPU1).
    ///
    /// Writing 1 to a bit in `CPU1_IPC_ICR` clears the pending flag for that
    /// channel. Typically called from D0's IPC interrupt handler.
    pub fn clear_d0(&mut self, channels: ChannelMask) {
        self.regs()
            .cpu1_ipc_icr()
            .write(|w| unsafe { w.cpu1_ipc_icr().bits(channels) });
    }

    /// Set the interrupt-unmask for D0 channels.
    ///
    /// Writing 1 to a bit unmasks (enables) that channel's interrupt delivery
    /// to D0. Writing 0 keeps the channel masked.
    pub fn unmask_d0(&mut self, channels: ChannelMask) {
        self.regs()
            .cpu1_ipc_iusr()
            .write(|w| unsafe { w.bits(channels as u32) });
    }

    // -----------------------------------------------------------------------
    // Signalling toward M0 (CPU0)
    // -----------------------------------------------------------------------

    /// Raise an interrupt on M0 (CPU0) for the given channel mask.
    ///
    /// Writing 1 to a bit in `CPU0_IPC_ISWR` sets the corresponding pending
    /// bit in M0's interrupt-status register.
    pub fn trigger_m0(&mut self, channels: ChannelMask) {
        self.regs()
            .cpu0_ipc_iswr()
            .write(|w| unsafe { w.cpu0_ipc_iswr().bits(channels) });
    }

    /// Read the raw interrupt-pending status for M0 (CPU0).
    ///
    /// Each set bit indicates that D0 has signalled that channel to M0.
    /// Call [`clear_m0`] to acknowledge.
    pub fn pending_m0(&self) -> ChannelMask {
        self.regs()
            .cpu0_ipc_irsrr()
            .read()
            .bits() as ChannelMask
    }

    /// Clear the pending interrupt(s) on M0 (CPU0).
    ///
    /// Writing 1 to a bit in `CPU0_IPC_ICR` clears the pending flag for that
    /// channel. Typically called from M0's IPC interrupt handler.
    pub fn clear_m0(&mut self, channels: ChannelMask) {
        self.regs()
            .cpu0_ipc_icr()
            .write(|w| unsafe { w.cpu0_ipc_icr().bits(channels) });
    }

    /// Set the interrupt-unmask for M0 channels.
    ///
    /// Writing 1 to a bit unmasks (enables) that channel's interrupt delivery
    /// to M0. Writing 0 keeps the channel masked.
    pub fn unmask_m0(&mut self, channels: ChannelMask) {
        self.regs()
            .cpu0_ipc_iusr()
            .write(|w| unsafe { w.bits(channels as u32) });
    }

    // -----------------------------------------------------------------------
    // Self-interrupt (for testing / software IRQ)
    // -----------------------------------------------------------------------

    /// Raise a software interrupt on the current core.
    pub fn trigger_self(&mut self, channels: ChannelMask) {
        #[cfg(feature = "d0")]
        self.trigger_d0(channels);
        #[cfg(any(feature = "m0", feature = "lp"))]
        self.trigger_m0(channels);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::ipc::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_IPC_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_ipc_trigger_d0_writes_cpu1_iswr() {
        let regs = setup();
        let mut ipc = Ipc::new(unsafe { PacIpc::steal() });

        ipc.trigger_d0(0x0001); // channel 0

        let val = unsafe { core::ptr::read_volatile(regs.cpu1_ipc_iswr().as_ptr()) };
        assert_eq!(val & 0xffff, 0x0001, "CPU1_IPC_ISWR bit 0 should be set");
    }

    #[test]
    fn test_ipc_clear_d0_writes_cpu1_icr() {
        let regs = setup();
        let mut ipc = Ipc::new(unsafe { PacIpc::steal() });

        ipc.clear_d0(0x00FF); // clear channels 0-7

        let val = unsafe { core::ptr::read_volatile(regs.cpu1_ipc_icr().as_ptr()) };
        assert_eq!(val & 0xffff, 0x00FF, "CPU1_IPC_ICR bits 0-7 should be written");
    }

    #[test]
    fn test_ipc_pending_d0_reads_cpu1_irsrr() {
        let regs = setup();

        unsafe { core::ptr::write_volatile(regs.cpu1_ipc_irsrr().as_ptr(), 0x0055) };

        let ipc = Ipc::new(unsafe { PacIpc::steal() });

        assert_eq!(ipc.pending_d0(), 0x0055, "Pending channels should reflect CPU1_IPC_IRSRR");
    }

    #[test]
    fn test_ipc_unmask_d0_writes_cpu1_iusr() {
        let regs = setup();
        let mut ipc = Ipc::new(unsafe { PacIpc::steal() });

        ipc.unmask_d0(0x0F0F);

        let val = unsafe { core::ptr::read_volatile(regs.cpu1_ipc_iusr().as_ptr()) };
        assert_eq!(val & 0xffff, 0x0F0F, "CPU1 IPC channels should be unmasked");
    }

    #[test]
    fn test_ipc_trigger_m0_writes_cpu0_iswr() {
        let regs = setup();
        let mut ipc = Ipc::new(unsafe { PacIpc::steal() });

        ipc.trigger_m0(0x0002); // channel 1

        let val = unsafe { core::ptr::read_volatile(regs.cpu0_ipc_iswr().as_ptr()) };
        assert_eq!(val & 0xffff, 0x0002, "CPU0_IPC_ISWR bit 1 should be set");
    }

    #[test]
    fn test_ipc_clear_m0_writes_cpu0_icr() {
        let regs = setup();
        let mut ipc = Ipc::new(unsafe { PacIpc::steal() });

        ipc.clear_m0(0x00FF); // clear channels 0-7

        let val = unsafe { core::ptr::read_volatile(regs.cpu0_ipc_icr().as_ptr()) };
        assert_eq!(val & 0xffff, 0x00FF, "CPU0_IPC_ICR bits 0-7 should be written");
    }

    #[test]
    fn test_ipc_pending_m0_reads_cpu0_irsrr() {
        let regs = setup();

        unsafe { core::ptr::write_volatile(regs.cpu0_ipc_irsrr().as_ptr(), 0x0042) };

        let ipc = Ipc::new(unsafe { PacIpc::steal() });

        assert_eq!(ipc.pending_m0(), 0x0042, "Pending channels should reflect CPU0_IPC_IRSRR");
    }

    #[test]
    fn test_ipc_unmask_m0_writes_cpu0_iusr() {
        let regs = setup();
        let mut ipc = Ipc::new(unsafe { PacIpc::steal() });

        ipc.unmask_m0(0xFFFF); // unmask all channels

        let val = unsafe { core::ptr::read_volatile(regs.cpu0_ipc_iusr().as_ptr()) };
        assert_eq!(val & 0xffff, 0xFFFF, "All CPU0 IPC channels should be unmasked");
    }
}
