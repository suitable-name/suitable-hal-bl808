//! wifi_m0 — BL808 E907 (M0) WiFi coexistence + IPC polling example
//!
//! Demonstrates peripheral init and IPC-driven state polling on the M0 core.
//!
//! # Architecture
//!
//! The BL808 WiFi MAC firmware runs as a blob on the M0 core.  The
//! application layer (this file) sits on top and communicates with the MAC
//! through a shared-memory IPC ring backed by the hardware IPC peripheral.
//!
//!   1. Enable coexistence arbitration (BT / WiFi time-sharing).
//!   2. Poll IPC until the MAC signals ready (D0→M0 channel 0).
//!   3. Trigger a passive scan on all 2.4 GHz channels (M0→D0 channel 0).
//!   4. Poll until scan-done (D0→M0 channel 1), then repeat every ~5 s.
//!
//! # Async runtime
//!
//! Uses `async-executor` (no_std executor) with a hand-rolled bare-metal poll
//! loop.  `futures-lite::future::block_on` requires the `std` feature and
//! cannot be used in firmware; instead we poll the top-level future directly
//! with a no-op waker, spinning until it completes.
//!
//! `futures_lite::future::yield_now` is a pure Future combinator that does
//! not need `std` — it is used inside async helpers to give other tasks a
//! chance to run.
//!
//! # HAL API (verified against current source)
//!
//! - `WifiDriver::new(dp.wifi)` — `bl808_pac::Wifi` implements `WifiInstance`.
//! - `WifiDriver::enable_coex()` — sets bit 0 of `coex_control_0`.
//! - `WifiDriver::get_coex_state() -> u32` — reads `coex_state`.
//! - `Ipc::new(dp.ipc)` — takes ownership of the PAC `Ipc` token.
//! - `Ipc::unmask_m0(mask: u16)` — writes `CPU0_IPC_IUSR`.
//! - `Ipc::pending_m0() -> u16` — reads `CPU0_IPC_IRSRR`.
//! - `Ipc::clear_m0(mask: u16)` — writes `CPU0_IPC_ICR` (W1C).
//! - `Ipc::trigger_d0(mask: u16)` — writes `CPU1_IPC_ISWR`.

#![no_std]
#![no_main]
// On the host target (used by `cargo clippy` in the workspace) the `#[entry]`
// macro is not wired to the real program entry, so Clippy sees everything
// called from `main` as dead code.  This is expected for no_std firmware.
#![cfg_attr(not(target_arch = "riscv32"), allow(dead_code))]

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

// `Wifi` must be in scope: it is used as a type argument in `wifi_task`.
use bl808_pac::{Peripherals, Wifi};
use bl808_hal::{
    ipc::Ipc,
    wifi::WifiDriver,
};
use panic_halt as _;
use riscv_rt::entry;

// ---------------------------------------------------------------------------
// IPC channel assignments (convention between M0 application and MAC blob).
// ---------------------------------------------------------------------------

/// D0→M0 channel set by the MAC firmware when its init sequence completes.
const CH_MAC_READY: u16 = 1 << 0;
/// M0→D0 channel used to request a background scan.
const CH_SCAN_REQ: u16 = 1 << 0;
/// D0→M0 channel set by the MAC when a scan cycle is done.
const CH_SCAN_DONE: u16 = 1 << 1;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[entry]
fn main() -> ! {
    let dp = Peripherals::take().unwrap();

    let mut wifi = WifiDriver::new(dp.wifi);
    let mut ipc = Ipc::new(dp.ipc);

    // Unmask the D0→M0 IPC channels we care about before enabling delivery.
    ipc.unmask_m0(CH_MAC_READY | CH_SCAN_DONE);

    // Enable coexistence arbitration (BT/WiFi RF time-sharing).
    wifi.enable_coex();

    // Drive the top-level async task to completion using a bare-metal spin
    // loop.  We cannot use `futures_lite::future::block_on` because it
    // requires the `std` feature (it parks the OS thread).  Instead we pin
    // the future on the stack and poll it with a no-op waker — appropriate
    // for single-core bare-metal where there is no OS thread to yield to.
    let mut task = pin!(wifi_task(&mut wifi, &mut ipc));
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    loop {
        match task.as_mut().poll(&mut cx) {
            Poll::Ready(()) => break,
            Poll::Pending => core::hint::spin_loop(),
        }
    }

    // Should not be reached; park the core.
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Top-level async task
// ---------------------------------------------------------------------------

async fn wifi_task(wifi: &mut WifiDriver<Wifi>, ipc: &mut Ipc) {
    // Phase 1: wait for the MAC to finish its self-test and PLL calibration.
    wait_mac_ready(wifi, ipc).await;

    // Phase 2: continuous scan loop — never returns in normal operation.
    loop {
        // Signal the MAC to start a passive scan.
        ipc.trigger_d0(CH_SCAN_REQ);

        // Yield until the MAC reports scan-done.
        wait_ipc_channel(ipc, CH_SCAN_DONE).await;

        // Acknowledge (clear) the scan-done channel.
        ipc.clear_m0(CH_SCAN_DONE);

        // BSS entries are in the shared SRAM region agreed at link time;
        // a real driver would iterate them here before the next scan.

        // Wait ~5 s before the next scan.
        spin_ms(5_000);
    }
}

// ---------------------------------------------------------------------------
// Phase helpers
// ---------------------------------------------------------------------------

/// Wait until both the hardware coex-state bit and the MAC-ready IPC channel
/// are asserted, then clear the IPC pending bit.
async fn wait_mac_ready(wifi: &mut WifiDriver<Wifi>, ipc: &mut Ipc) {
    loop {
        // Bit 0 of coex_state is set by the MAC when it is ready.
        let hw_ready = wifi.get_coex_state() & 1 != 0;
        let ipc_ready = ipc.pending_m0() & CH_MAC_READY != 0;

        if hw_ready && ipc_ready {
            ipc.clear_m0(CH_MAC_READY);
            return;
        }

        yield_now().await;
    }
}

/// Yield until the specified D0→M0 IPC channel has a pending interrupt.
async fn wait_ipc_channel(ipc: &mut Ipc, mask: u16) {
    loop {
        if ipc.pending_m0() & mask != 0 {
            return;
        }
        yield_now().await;
    }
}

/// A lightweight future that yields control back to the poll loop once.
struct YieldNow(bool);

fn yield_now() -> YieldNow {
    YieldNow(false)
}

impl Future for YieldNow {
    type Output = ();
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Spin-delay for approximately `ms` milliseconds.
///
/// Calibrated for a 160 MHz E907 clock; adjust `LOOPS_PER_MS` for the actual
/// operating frequency.
#[allow(dead_code)] // called from async context; host Clippy cannot trace that
fn spin_ms(ms: u32) {
    const LOOPS_PER_MS: u32 = 40_000; // ~160 MHz / 4 cycles per spin_loop hint
    for _ in 0..ms {
        for _ in 0..LOOPS_PER_MS {
            core::hint::spin_loop();
        }
    }
}

// ---------------------------------------------------------------------------
// No-op waker for the bare-metal poll loop
// ---------------------------------------------------------------------------

/// A waker that does nothing — appropriate for a spin-poll executor on
/// bare-metal where there is no event system to register wake-ups with.
fn noop_waker() -> Waker {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |p| RawWaker::new(p, &VTABLE), // clone
        |_| {},                         // wake
        |_| {},                         // wake_by_ref
        |_| {},                         // drop
    );
    // SAFETY: The waker never dereferences the data pointer.
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}
