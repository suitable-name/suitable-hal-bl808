use core::cell::RefCell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::{Context, Poll, Waker};

use bl808_pac::Dma0;

// ── Error and configuration types ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaError {
    ChannelBusy,
    InvalidAlignment,
    TransferError,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferWidth {
    Bits8 = 0,
    Bits16 = 1,
    Bits32 = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BurstSize {
    Burst1 = 0,
    Burst4 = 1,
    Burst8 = 2,
    Burst16 = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlowControl {
    Mem2Mem = 0,
    Mem2Peri = 1,
    Peri2Mem = 2,
    Peri2Peri = 3,
}

/// Direction of a peripheral DMA transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    /// Memory → peripheral (e.g. I2S TX).
    MemToPeri,
    /// Peripheral → memory (e.g. I2S RX).
    PeriToMem,
}

/// Linked-list item for scatter-gather DMA transfers.
///
/// # Control-word bit layout (sourced from BL808 `dma_control_data_t` / PAC `control.rs`)
///
/// | Bits  | Field          | Description                            |
/// |-------|----------------|----------------------------------------|
/// | 11:0  | TransferSize   | Number of transfers (elements, ≤ 4095) |
/// | 13:12 | SBSize         | Source burst size (0=1,1=4,2=8,3=16)  |
/// | 16:15 | DBSize         | Dest   burst size (0=1,1=4,2=8,3=16)  |
/// | 19:18 | SWidth         | Source width (0=8-bit,1=16,2=32)      |
/// | 22:21 | DWidth         | Dest   width (0=8-bit,1=16,2=32)      |
/// | 26    | SI             | Source address increment               |
/// | 27    | DI             | Destination address increment          |
/// | 31    | I              | Terminal-count interrupt enable        |
///
/// Bits 14,17,20,23-25,28-30 (dst_min_mode, dst_add_mode, reserved, fix_cnt, prot)
/// are left zero for normal peripheral transfers.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct Lli {
    pub src_addr: u32,
    pub dst_addr: u32,
    pub next_lli: u32,
    pub control: u32,
}

impl Lli {
    /// Build a `control` word from individual fields.
    ///
    /// Bit positions are taken directly from the PAC `control.rs` (BL808 DMA channel
    /// Control register) and confirmed against the SDK `dma_control_data_t`.
    ///
    /// - `transfer_size`: number of *elements* (each element = one `width`-wide access), max 4095
    /// - `src_burst` / `dst_burst`: burst size code (see [`BurstSize`])
    /// - `src_width` / `dst_width`: bus width code (see [`TransferWidth`])
    /// - `src_inc`: increment the source address after each transfer
    /// - `dst_inc`: increment the destination address after each transfer
    /// - `irq`: enable the terminal-count interrupt on this LLI
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn build_control(
        transfer_size: u16,
        src_burst: BurstSize,
        dst_burst: BurstSize,
        src_width: TransferWidth,
        dst_width: TransferWidth,
        src_inc: bool,
        dst_inc: bool,
        irq: bool,
    ) -> u32 {
        let size = (transfer_size as u32) & 0x0FFF;   // bits [11:0]
        let sbsize = (src_burst as u32) & 0x3;         // bits [13:12]
        let dbsize = (dst_burst as u32) & 0x3;         // bits [16:15]
        let swidth = (src_width as u32) & 0x3;         // bits [19:18]
        let dwidth = (dst_width as u32) & 0x3;         // bits [22:21]
        let si = src_inc as u32;                        // bit  [26]
        let di = dst_inc as u32;                        // bit  [27]
        let i  = irq as u32;                            // bit  [31]

        size
            | (sbsize << 12)
            | (dbsize << 15)
            | (swidth << 18)
            | (dwidth << 21)
            | (si << 26)
            | (di << 27)
            | (i  << 31)
    }
}

/// Build a circular LLI ring for continuous peripheral DMA (e.g. I2S audio streaming).
///
/// Fills a caller-provided `ring` slice with one [`Lli`] per segment.  Each
/// LLI's `next_lli` points to the next item, and the **last** item's
/// `next_lli` wraps back to `ring[0]`, forming a true circular list.
///
/// The terminal-count interrupt bit is set on every LLI so the application
/// receives a callback once per buffer.
///
/// # Parameters
/// - `ring`      – caller-allocated slice of [`Lli`]; length must equal `segments.len()`
/// - `segments`  – list of `(buf_addr, element_count)` pairs, one per LLI node.
///   `element_count` must not exceed 4095.
/// - `fifo_addr` – physical address of the peripheral FIFO register
///   (`tx.tx_fifo_addr()` or `rx.rx_fifo_addr()`)
/// - `direction`     – [`DmaDirection::MemToPeri`] for TX, [`DmaDirection::PeriToMem`] for RX
/// - `width`         – bus width used for each transfer element
/// - `burst`         – burst size (must match the peripheral FIFO threshold)
///
/// # Panics
/// Panics if `ring.len() != segments.len()` or `ring.is_empty()`.
///
/// # Example
/// ```ignore
/// use bl808_hal::dma::{build_circular_lli, BurstSize, DmaDirection, Lli, TransferWidth};
///
/// const HALF: usize = 512;
/// static mut BUF_A: [u32; HALF] = [0u32; HALF];
/// static mut BUF_B: [u32; HALF] = [0u32; HALF];
/// static mut RING: [Lli; 2] = [Lli { src_addr: 0, dst_addr: 0, next_lli: 0, control: 0 }; 2];
///
/// // Configure I2S TX DMA enable + FIFO threshold.
/// tx.start_tx_dma(3);
///
/// let segs = unsafe {
///     [(BUF_A.as_ptr() as u32, HALF as u16),
///      (BUF_B.as_ptr() as u32, HALF as u16)]
/// };
/// let fifo = tx.tx_fifo_addr();
/// unsafe {
///     build_circular_lli(
///         &mut RING,
///         &segs,
///         fifo,
///         DmaDirection::MemToPeri,
///         TransferWidth::Bits32,
///         BurstSize::Burst4,
///     );
///     // Program the DMA channel: flow_cntrl = M2P, dst_peripheral = I2S_TX (17).
///     dma_channel.start_lli_peripheral(
///         &RING[0],
///         FlowControl::Mem2Peri,
///         0,   // src_peripheral (unused for M2P)
///         17,  // dst_peripheral = I2S TX (DMA_REQ_I2S_TX = 17)
///     ).unwrap();
/// }
/// ```
pub fn build_circular_lli(
    ring: &mut [Lli],
    segments: &[(u32, u16)],
    fifo_addr: u32,
    direction: DmaDirection,
    width: TransferWidth,
    burst: BurstSize,
) {
    assert!(!ring.is_empty(), "LLI ring must not be empty");
    assert_eq!(
        ring.len(),
        segments.len(),
        "ring.len() must equal segments.len()"
    );

    let n = ring.len();
    for (i, ((buf_addr, count), slot)) in segments.iter().zip(ring.iter_mut()).enumerate() {
        // For TX (Mem→Peri): memory is source (SI=1), FIFO is dest (DI=0).
        // For RX (Peri→Mem): FIFO is source (SI=0), memory is dest (DI=1).
        let (src, dst, src_inc, dst_inc) = match direction {
            DmaDirection::MemToPeri => (*buf_addr, fifo_addr, true, false),
            DmaDirection::PeriToMem => (fifo_addr, *buf_addr, false, true),
        };

        slot.src_addr = src;
        slot.dst_addr = dst;
        // next_lli will be patched below to the physical address of the next item
        // (or wrap-around).  We store the *index* temporarily and fix up after.
        slot.next_lli = 0;
        slot.control = Lli::build_control(
            *count, burst, burst, width, width,
            src_inc, dst_inc,
            true, // terminal-count interrupt on every buffer
        );
        // We need the actual address of each Lli.  Compute it from the slice base.
        let _ = i; // silence unused-variable warning; patched below
    }

    // Patch next_lli pointers.
    // We must read the address of each slot *after* writing all fields to
    // avoid UB with the borrow checker — do a second pass.
    for i in 0..n {
        let next_idx = (i + 1) % n;
        let next_addr = &ring[next_idx] as *const Lli as u32;
        ring[i].next_lli = next_addr;
    }
}

// ── Async transfer state ──────────────────────────────────────────────────────

const STATE_IDLE: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_DONE: u8 = 2;
const STATE_ERROR: u8 = 3;

/// Transfer-completion state, one entry per DMA0 channel (0-7).
static ASYNC_STATE: [AtomicU8; 8] = [const { AtomicU8::new(STATE_IDLE) }; 8];

/// Waker storage, one entry per DMA0 channel.  Access is always inside a
/// critical section, so `RefCell` (non-Sync) is safe here through
/// `critical_section::Mutex` (which implements `Sync` for `Send` payloads).
static ASYNC_WAKERS: critical_section::Mutex<RefCell<[Option<Waker>; 8]>> =
    critical_section::Mutex::new(RefCell::new([None, None, None, None, None, None, None, None]));

// ── Mock pointer for unit tests ───────────────────────────────────────────────

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_DMA0_PTR: core::cell::Cell<*const bl808_pac::dma0::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

#[inline]
fn dma0_regs() -> &'static bl808_pac::dma0::RegisterBlock {
    #[cfg(not(test))]
    return unsafe { &*Dma0::PTR };
    #[cfg(test)]
    return MOCK_DMA0_PTR.with(|p| unsafe { &*p.get() });
}

// ── DmaInstance trait ─────────────────────────────────────────────────────────

pub trait DmaInstance {
    fn ptr() -> *const bl808_pac::dma0::RegisterBlock;
}

impl DmaInstance for Dma0 {
    fn ptr() -> *const bl808_pac::dma0::RegisterBlock {
        #[cfg(not(test))]
        return Dma0::PTR;
        #[cfg(test)]
        return MOCK_DMA0_PTR.with(|p| p.get());
    }
}

#[cfg(any(feature = "m0", feature = "lp"))]
impl DmaInstance for bl808_pac::Dma1 {
    fn ptr() -> *const bl808_pac::dma0::RegisterBlock {
        bl808_pac::Dma1::PTR
    }
}

#[cfg(feature = "d0")]
impl DmaInstance for bl808_pac::Dma2 {
    fn ptr() -> *const bl808_pac::dma0::RegisterBlock {
        bl808_pac::Dma2::PTR
    }
}

// ── DMA controller wrapper ────────────────────────────────────────────────────

pub struct Dma<DMA> {
    _dma: DMA,
}

impl<DMA> Dma<DMA>
where
    DMA: DmaInstance,
{
    pub fn new(dma: DMA) -> Self {
        let regs = unsafe { &*DMA::ptr() };
        regs.top_config().modify(|_, w| w.e().set_bit());
        Self { _dma: dma }
    }

    pub fn split(self) -> Channels<DMA> {
        Channels {
            ch0: Some(Channel { _marker: PhantomData }),
            ch1: Some(Channel { _marker: PhantomData }),
            ch2: Some(Channel { _marker: PhantomData }),
            ch3: Some(Channel { _marker: PhantomData }),
            ch4: Some(Channel { _marker: PhantomData }),
            ch5: Some(Channel { _marker: PhantomData }),
            ch6: Some(Channel { _marker: PhantomData }),
            ch7: Some(Channel { _marker: PhantomData }),
        }
    }
}

pub struct Channels<DMA> {
    pub ch0: Option<Channel<DMA, 0>>,
    pub ch1: Option<Channel<DMA, 1>>,
    pub ch2: Option<Channel<DMA, 2>>,
    pub ch3: Option<Channel<DMA, 3>>,
    pub ch4: Option<Channel<DMA, 4>>,
    pub ch5: Option<Channel<DMA, 5>>,
    pub ch6: Option<Channel<DMA, 6>>,
    pub ch7: Option<Channel<DMA, 7>>,
}

// ── Per-channel API ───────────────────────────────────────────────────────────

pub struct Channel<DMA, const N: u8> {
    _marker: PhantomData<DMA>,
}

impl<DMA, const N: u8> Channel<DMA, N>
where
    DMA: DmaInstance,
{
    fn regs(&self) -> &bl808_pac::dma0::Ch {
        let dma = unsafe { &*DMA::ptr() };
        dma.ch(N as usize)
    }

    pub fn is_busy(&self) -> bool {
        let dma = unsafe { &*DMA::ptr() };
        (dma.enbld_chns().read().bits() & (1 << N)) != 0
    }

    pub fn stop(&mut self) {
        self.regs().config().modify(|_, w| w.e().clear_bit());
    }

    /// Blocking poll until the current transfer completes (or times out).
    pub fn wait_transfer(&self) -> Result<(), DmaError> {
        let dma = unsafe { &*DMA::ptr() };
        let mut timeout = 10_000_000u32;

        while self.is_busy() {
            timeout -= 1;
            if timeout == 0 {
                return Err(DmaError::Timeout);
            }
            core::hint::spin_loop();
        }

        if (dma.int_error_status().read().bits() & (1 << N)) != 0 {
            dma.int_err_clr().write(|w| unsafe { w.bits(1 << N) });
            return Err(DmaError::TransferError);
        }

        dma.int_tcclear().write(|w| unsafe { w.bits(1 << N) });
        Ok(())
    }

    /// Start a memory-to-memory DMA transfer (non-blocking).
    ///
    /// # Safety
    /// `src` and `dst` must remain valid for the entire duration of the
    /// transfer.  Call [`wait_transfer`] or await [`transfer_mem2mem_async`]
    /// before dropping them.
    pub unsafe fn transfer_mem2mem<T>(
        &mut self,
        src: &[T],
        dst: &mut [T],
        width: TransferWidth,
        burst: BurstSize,
    ) -> Result<(), DmaError> {
        if self.is_busy() {
            return Err(DmaError::ChannelBusy);
        }

        let len = src.len().min(dst.len()) as u16;
        if len == 0 {
            return Ok(());
        }

        self.stop();
        let regs = self.regs();

        regs.src_addr().write(|w| unsafe { w.bits(src.as_ptr() as u32) });
        regs.dst_addr().write(|w| unsafe { w.bits(dst.as_mut_ptr() as u32) });
        regs.lli().write(|w| unsafe { w.bits(0) });

        regs.control().write(|w| unsafe {
            w.transfer_size()
                .bits(len)
                .sbsize()
                .bits(burst as u8)
                .dbsize()
                .bits(burst as u8)
                .swidth()
                .bits(width as u8)
                .dwidth()
                .bits(width as u8)
                .si()
                .set_bit()
                .di()
                .set_bit()
                .i()
                .set_bit()
        });

        regs.config().write(|w| unsafe {
            w.e().set_bit().flow_cntrl().bits(FlowControl::Mem2Mem as u8)
        });

        Ok(())
    }

    /// Start a linked-list DMA transfer (non-blocking).
    ///
    /// Loads the first [`Lli`] into the DMA channel registers and enables the
    /// channel.  The hardware follows `next_lli` pointers automatically.
    ///
    /// This variant does **not** touch `flow_cntrl` or the peripheral IDs —
    /// use it for memory-to-memory LLI chains.  For peripheral transfers,
    /// call [`start_lli_peripheral`] instead.
    ///
    /// # Safety
    /// All `Lli` items in the chain (and their source/destination buffers)
    /// must remain valid until the transfer completes.
    pub unsafe fn start_lli(&mut self, lli: &Lli) -> Result<(), DmaError> {
        if self.is_busy() {
            return Err(DmaError::ChannelBusy);
        }

        self.stop();
        let regs = self.regs();

        regs.src_addr().write(|w| unsafe { w.bits(lli.src_addr) });
        regs.dst_addr().write(|w| unsafe { w.bits(lli.dst_addr) });
        regs.lli().write(|w| unsafe { w.bits(lli.next_lli) });
        regs.control().write(|w| unsafe { w.bits(lli.control) });

        regs.config().modify(|_, w| w.e().set_bit());
        Ok(())
    }

    /// Start a linked-list peripheral DMA transfer (non-blocking).
    ///
    /// Like [`start_lli`] but also configures the channel's `FlowCntrl`,
    /// `SrcPeripheral`, and `DstPeripheral` fields — required for any
    /// memory↔peripheral transfer.
    ///
    /// For I2S:
    /// - TX (Mem→Peri): `flow = FlowControl::Mem2Peri`, `src_periph = 0`,
    ///   `dst_periph = 17` (`DMA_REQ_I2S_TX`)
    /// - RX (Peri→Mem): `flow = FlowControl::Peri2Mem`, `src_periph = 16`
    ///   (`DMA_REQ_I2S_RX`), `dst_periph = 0`
    ///
    /// Peripheral IDs are defined in the BL808 SDK `hal_dma.h`
    /// (`DMA_REQUEST_I2S_TX = 17`, `DMA_REQUEST_I2S_RX = 16`).
    ///
    /// # Safety
    /// All `Lli` items in the chain (and their source/destination buffers)
    /// must remain valid until the transfer is stopped.  For circular LLI
    /// rings this means for the lifetime of the audio stream.
    pub unsafe fn start_lli_peripheral(
        &mut self,
        lli: &Lli,
        flow: FlowControl,
        src_periph: u8,
        dst_periph: u8,
    ) -> Result<(), DmaError> {
        if self.is_busy() {
            return Err(DmaError::ChannelBusy);
        }

        self.stop();
        let regs = self.regs();

        regs.src_addr().write(|w| unsafe { w.bits(lli.src_addr) });
        regs.dst_addr().write(|w| unsafe { w.bits(lli.dst_addr) });
        regs.lli().write(|w| unsafe { w.bits(lli.next_lli) });
        regs.control().write(|w| unsafe { w.bits(lli.control) });

        regs.config().write(|w| unsafe {
            w.e()
                .set_bit()
                .flow_cntrl()
                .bits(flow as u8)
                .src_peripheral()
                .bits(src_periph)
                .dst_peripheral()
                .bits(dst_periph)
        });
        Ok(())
    }
}

// ── Async transfer path (DMA0, channels 0-7) ─────────────────────────────────
//
// Pattern:
//   1. Caller calls `transfer_mem2mem_async`, which arms the state machine and
//      starts the DMA.
//   2. The executor polls `TransferFuture`.  If the DMA has not finished, the
//      future registers a `Waker` and returns `Poll::Pending`.
//   3. The interrupt handler `on_dma0_tc_interrupt` is called from the
//      platform interrupt vector.  It reads the PAC status registers, marks
//      the appropriate channel DONE/ERROR, and calls `Waker::wake`.
//   4. The executor re-polls the future, which now returns `Poll::Ready`.

impl<const N: u8> Channel<Dma0, N> {
    /// Async memory-to-memory transfer on DMA0 channel `N`.
    ///
    /// Returns a [`TransferFuture`] that resolves once the transfer completes
    /// (signalled via `on_dma0_tc_interrupt`).
    ///
    /// # Safety
    /// `src` and `dst` must remain valid until the returned future resolves.
    pub unsafe fn transfer_mem2mem_async<T>(
        &mut self,
        src: &[T],
        dst: &mut [T],
        width: TransferWidth,
        burst: BurstSize,
    ) -> Result<TransferFuture, DmaError> {
        // Mark channel as running before starting hardware so the interrupt
        // handler can never observe a transition from IDLE to DONE.
        ASYNC_STATE[N as usize].store(STATE_RUNNING, Ordering::Release);

        // SAFETY: caller guarantees buffer lifetimes.
        unsafe { self.transfer_mem2mem(src, dst, width, burst) }?;

        Ok(TransferFuture { channel: N })
    }
}

/// Future returned by [`Channel::transfer_mem2mem_async`].
///
/// Resolves to `Ok(())` on terminal-count or `Err(DmaError::TransferError)`
/// on a DMA error interrupt.
pub struct TransferFuture {
    channel: u8,
}

impl core::future::Future for TransferFuture {
    type Output = Result<(), DmaError>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ch = self.channel as usize;

        // Fast path: transfer already finished.
        match ASYNC_STATE[ch].load(Ordering::Acquire) {
            STATE_DONE => {
                ASYNC_STATE[ch].store(STATE_IDLE, Ordering::Release);
                return Poll::Ready(Ok(()));
            }
            STATE_ERROR => {
                ASYNC_STATE[ch].store(STATE_IDLE, Ordering::Release);
                return Poll::Ready(Err(DmaError::TransferError));
            }
            _ => {}
        }

        // Register waker so the interrupt handler can re-schedule this task.
        critical_section::with(|cs| {
            ASYNC_WAKERS.borrow(cs).borrow_mut()[ch] = Some(cx.waker().clone());
        });

        // Re-check state after storing the waker to close the race where the
        // interrupt fires between the first check and waker registration.
        match ASYNC_STATE[ch].load(Ordering::Acquire) {
            STATE_DONE => {
                ASYNC_STATE[ch].store(STATE_IDLE, Ordering::Release);
                Poll::Ready(Ok(()))
            }
            STATE_ERROR => {
                ASYNC_STATE[ch].store(STATE_IDLE, Ordering::Release);
                Poll::Ready(Err(DmaError::TransferError))
            }
            _ => Poll::Pending,
        }
    }
}

/// Call this from the DMA0 terminal-count / error interrupt handler.
///
/// Reads the PAC interrupt-status registers, clears all pending flags, sets
/// the per-channel async state, and wakes any registered task waker.
pub fn on_dma0_tc_interrupt() {
    let dma = dma0_regs();
    let tc = dma.int_tcstatus().read().bits();
    let err = dma.int_error_status().read().bits();
    let active = tc | err;
    if active == 0 {
        return;
    }

    // Clear all pending flags before waking tasks.
    dma.int_tcclear().write(|w| unsafe { w.bits(tc) });
    dma.int_err_clr().write(|w| unsafe { w.bits(err) });

    for ch in 0u8..8 {
        let mask = 1u32 << ch;
        if active & mask == 0 {
            continue;
        }
        let state = if (err & mask) != 0 { STATE_ERROR } else { STATE_DONE };
        ASYNC_STATE[ch as usize].store(state, Ordering::Release);

        let waker = critical_section::with(|cs| {
            ASYNC_WAKERS.borrow(cs).borrow_mut()[ch as usize].take()
        });
        if let Some(w) = waker {
            w.wake();
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::Future;
    use core::pin::pin;
    use std::boxed::Box;

    // The async DMA tests share the global ASYNC_STATE / ASYNC_WAKERS statics.
    // Without serialization they race under the parallel test runner: one test's
    // interrupt can consume another's wake, leaving `block_on` waiting forever.
    // Serialize them through a single lock (ignoring poisoning so one failing
    // test does not cascade into the others).
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // Mock buffer layout:
    //   header: 0x00..0x100 (64 u32s: global DMA registers + reserved)
    //   ch0:    0x100..0x200 (64 u32s)
    //   ...
    //   ch7:    0x800..0x900 (64 u32s)
    // Total: 0x900 bytes = 576 u32s
    fn make_mock() -> *const bl808_pac::dma0::RegisterBlock {
        let buf = Box::leak(Box::new([0u32; 576]));
        let ptr = buf.as_ptr() as *const bl808_pac::dma0::RegisterBlock;
        MOCK_DMA0_PTR.with(|p| p.set(ptr));
        ptr
    }

    /// Write a u32 at a byte offset into the mock register block.
    unsafe fn mock_write(ptr: *const bl808_pac::dma0::RegisterBlock, byte_offset: usize, val: u32) {
        let base = ptr as *mut u32;
        base.add(byte_offset / 4).write_volatile(val);
    }

    /// Read a u32 at a byte offset from the mock register block.
    unsafe fn mock_read(ptr: *const bl808_pac::dma0::RegisterBlock, byte_offset: usize) -> u32 {
        let base = ptr as *const u32;
        base.add(byte_offset / 4).read_volatile()
    }

    // Minimal no-op waker for manual Future polling.
    fn noop_waker() -> core::task::Waker {
        use core::task::{RawWaker, RawWakerVTable, Waker};
        fn no_op(_: *const ()) {}
        fn clone(data: *const ()) -> RawWaker {
            RawWaker::new(data, &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }

    #[test]
    fn test_interrupt_handler_sets_done_state() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mock = make_mock();
        // Simulate channel 0 TC interrupt (int_tcstatus at offset 0x04).
        unsafe { mock_write(mock, 0x04, 0b0000_0001) };
        ASYNC_STATE[0].store(STATE_RUNNING, Ordering::Release);

        on_dma0_tc_interrupt();

        assert_eq!(ASYNC_STATE[0].load(Ordering::Acquire), STATE_DONE);
        // int_tcclear (offset 0x08) should have been written.
        let cleared = unsafe { mock_read(mock, 0x08) };
        assert_eq!(cleared, 0b0000_0001);
    }

    #[test]
    fn test_interrupt_handler_sets_error_state() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mock = make_mock();
        // Simulate channel 2 error (int_error_status at offset 0x0c).
        unsafe { mock_write(mock, 0x0c, 1 << 2) };
        ASYNC_STATE[2].store(STATE_RUNNING, Ordering::Release);

        on_dma0_tc_interrupt();

        assert_eq!(ASYNC_STATE[2].load(Ordering::Acquire), STATE_ERROR);
        let cleared = unsafe { mock_read(mock, 0x10) }; // int_err_clr offset 0x10
        assert_eq!(cleared, 1 << 2);
    }

    #[test]
    fn test_interrupt_handler_noop_when_no_flags() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mock = make_mock();
        // No TC or error bits set.
        unsafe { mock_write(mock, 0x04, 0) };
        unsafe { mock_write(mock, 0x0c, 0) };
        ASYNC_STATE[0].store(STATE_IDLE, Ordering::Release);

        on_dma0_tc_interrupt();

        // State should remain IDLE.
        assert_eq!(ASYNC_STATE[0].load(Ordering::Acquire), STATE_IDLE);
    }

    #[test]
    fn test_transfer_future_ready_after_interrupt() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mock = make_mock();

        // Pre-arm state as RUNNING, then fire the interrupt.
        ASYNC_STATE[0].store(STATE_RUNNING, Ordering::Release);
        unsafe { mock_write(mock, 0x04, 0b0000_0001) };
        on_dma0_tc_interrupt();

        // The future should resolve on the first poll (state already DONE).
        let mut fut = pin!(TransferFuture { channel: 0 });
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let result = match fut.as_mut().poll(&mut cx) {
            Poll::Ready(r) => r,
            Poll::Pending => panic!("future should be ready after interrupt"),
        };
        assert_eq!(result, Ok(()));
        // State should be reset to IDLE.
        assert_eq!(ASYNC_STATE[0].load(Ordering::Acquire), STATE_IDLE);
    }

    #[test]
    fn test_transfer_future_error_after_interrupt() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mock = make_mock();

        ASYNC_STATE[3].store(STATE_RUNNING, Ordering::Release);
        unsafe { mock_write(mock, 0x0c, 1 << 3) }; // error on ch3
        on_dma0_tc_interrupt();

        let mut fut = pin!(TransferFuture { channel: 3 });
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let result = match fut.as_mut().poll(&mut cx) {
            Poll::Ready(r) => r,
            Poll::Pending => panic!("future should be ready after error interrupt"),
        };
        assert_eq!(result, Err(DmaError::TransferError));
        assert_eq!(ASYNC_STATE[3].load(Ordering::Acquire), STATE_IDLE);
    }

    #[test]
    fn test_transfer_future_pending_then_ready() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _mock = make_mock();

        // Put channel 1 into RUNNING state; do NOT fire interrupt yet.
        ASYNC_STATE[1].store(STATE_RUNNING, Ordering::Release);
        let mut fut = pin!(TransferFuture { channel: 1 });

        // First poll: should return Pending.
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Pending));

        // Now fire the TC interrupt for channel 1.
        let mock = MOCK_DMA0_PTR.with(|p| p.get());
        unsafe { mock_write(mock, 0x04, 1 << 1) };
        on_dma0_tc_interrupt();

        // Second poll: should return Ready(Ok(())).
        assert!(matches!(fut.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }

    #[test]
    fn test_multi_channel_interrupt() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mock = make_mock();

        ASYNC_STATE[0].store(STATE_RUNNING, Ordering::Release);
        ASYNC_STATE[5].store(STATE_RUNNING, Ordering::Release);
        // TC on channels 0 and 5 simultaneously.
        unsafe { mock_write(mock, 0x04, (1 << 0) | (1 << 5)) };

        on_dma0_tc_interrupt();

        assert_eq!(ASYNC_STATE[0].load(Ordering::Acquire), STATE_DONE);
        assert_eq!(ASYNC_STATE[5].load(Ordering::Acquire), STATE_DONE);
        // Other channels should be untouched.
        assert_eq!(ASYNC_STATE[3].load(Ordering::Acquire), STATE_IDLE);
    }

    // ── LLI ring builder tests ────────────────────────────────────────────────

    /// Helper: build a raw control word and verify individual bit fields.
    fn decode_control(ctrl: u32) -> (u16, u8, u8, u8, u8, bool, bool, bool) {
        let transfer_size = (ctrl & 0x0FFF) as u16;
        let sbsize        = ((ctrl >> 12) & 0x3) as u8;
        let dbsize        = ((ctrl >> 15) & 0x3) as u8;
        let swidth        = ((ctrl >> 18) & 0x3) as u8;
        let dwidth        = ((ctrl >> 21) & 0x3) as u8;
        let si            = (ctrl >> 26) & 1 != 0;
        let di            = (ctrl >> 27) & 1 != 0;
        let irq           = (ctrl >> 31) & 1 != 0;
        (transfer_size, sbsize, dbsize, swidth, dwidth, si, di, irq)
    }

    #[test]
    fn test_lli_build_control_tx() {
        // TX: 128 elements, burst-4, 32-bit, src-inc, no dst-inc, irq
        let ctrl = Lli::build_control(
            128,
            BurstSize::Burst4,
            BurstSize::Burst4,
            TransferWidth::Bits32,
            TransferWidth::Bits32,
            true,  // SI
            false, // DI (FIFO address fixed)
            true,  // IRQ
        );
        let (ts, sb, db, sw, dw, si, di, irq) = decode_control(ctrl);
        assert_eq!(ts, 128);
        assert_eq!(sb, BurstSize::Burst4 as u8);
        assert_eq!(db, BurstSize::Burst4 as u8);
        assert_eq!(sw, TransferWidth::Bits32 as u8);
        assert_eq!(dw, TransferWidth::Bits32 as u8);
        assert!(si,  "source increment must be set for TX");
        assert!(!di, "dest increment must be clear for TX (FIFO is fixed)");
        assert!(irq, "IRQ bit must be set");
    }

    #[test]
    fn test_lli_build_control_rx() {
        // RX: 64 elements, burst-1, 16-bit, no src-inc, dst-inc, irq
        let ctrl = Lli::build_control(
            64,
            BurstSize::Burst1,
            BurstSize::Burst1,
            TransferWidth::Bits16,
            TransferWidth::Bits16,
            false, // SI (FIFO address fixed)
            true,  // DI
            true,  // IRQ
        );
        let (ts, _sb, _db, sw, dw, si, di, irq) = decode_control(ctrl);
        assert_eq!(ts, 64);
        assert_eq!(sw, TransferWidth::Bits16 as u8);
        assert_eq!(dw, TransferWidth::Bits16 as u8);
        assert!(!si, "source increment must be clear for RX (FIFO is fixed)");
        assert!(di,  "dest increment must be set for RX");
        assert!(irq, "IRQ bit must be set");
    }

    #[test]
    fn test_build_circular_lli_tx_two_buffers() {
        // Two equal-sized TX buffers → 2-element ring.
        let buf_a: u32 = 0x2200_0000;
        let buf_b: u32 = 0x2200_1000;
        let fifo:  u32 = 0x2000_AB88; // I2S TX FIFO (base + 0x88)
        let count: u16 = 256;

        let mut ring = [Lli::default(); 2];
        let segments = [(buf_a, count), (buf_b, count)];

        build_circular_lli(
            &mut ring,
            &segments,
            fifo,
            DmaDirection::MemToPeri,
            TransferWidth::Bits32,
            BurstSize::Burst4,
        );

        // (a) Circular: ring[0].next_lli → ring[1], ring[1].next_lli → ring[0]
        assert_eq!(
            ring[0].next_lli,
            &ring[1] as *const Lli as u32,
            "ring[0].next_lli should point to ring[1]"
        );
        assert_eq!(
            ring[1].next_lli,
            &ring[0] as *const Lli as u32,
            "ring[1].next_lli should wrap back to ring[0]"
        );

        // (b) TX: src = buf address, dst = fifo
        assert_eq!(ring[0].src_addr, buf_a, "ring[0] src should be buf_a");
        assert_eq!(ring[0].dst_addr, fifo,  "ring[0] dst should be FIFO");
        assert_eq!(ring[1].src_addr, buf_b, "ring[1] src should be buf_b");
        assert_eq!(ring[1].dst_addr, fifo,  "ring[1] dst should be FIFO");

        // (c) Control word: size, SI=1, DI=0, IRQ=1
        for (i, item) in ring.iter().enumerate() {
            let (ts, _sb, _db, _sw, _dw, si, di, irq) = decode_control(item.control);
            assert_eq!(ts, count, "ring[{i}] transfer_size mismatch");
            assert!(si,  "ring[{i}] SI should be set for TX");
            assert!(!di, "ring[{i}] DI should be clear for TX");
            assert!(irq, "ring[{i}] IRQ bit must be set");
        }
    }

    #[test]
    fn test_build_circular_lli_rx_two_buffers() {
        // Two equal-sized RX buffers → 2-element ring.
        let buf_a: u32 = 0x2300_0000;
        let buf_b: u32 = 0x2300_1000;
        let fifo:  u32 = 0x2000_AB8C; // I2S RX FIFO (base + 0x8C)
        let count: u16 = 128;

        let mut ring = [Lli::default(); 2];
        let segments = [(buf_a, count), (buf_b, count)];

        build_circular_lli(
            &mut ring,
            &segments,
            fifo,
            DmaDirection::PeriToMem,
            TransferWidth::Bits32,
            BurstSize::Burst4,
        );

        // (a) Circular
        assert_eq!(ring[0].next_lli, &ring[1] as *const Lli as u32);
        assert_eq!(ring[1].next_lli, &ring[0] as *const Lli as u32);

        // (b) RX: src = fifo, dst = buf address
        assert_eq!(ring[0].src_addr, fifo,  "ring[0] src should be FIFO for RX");
        assert_eq!(ring[0].dst_addr, buf_a, "ring[0] dst should be buf_a");
        assert_eq!(ring[1].src_addr, fifo,  "ring[1] src should be FIFO for RX");
        assert_eq!(ring[1].dst_addr, buf_b, "ring[1] dst should be buf_b");

        // (c) Control word: SI=0, DI=1, IRQ=1
        for (i, item) in ring.iter().enumerate() {
            let (ts, _sb, _db, _sw, _dw, si, di, irq) = decode_control(item.control);
            assert_eq!(ts, count, "ring[{i}] transfer_size mismatch");
            assert!(!si, "ring[{i}] SI should be clear for RX");
            assert!(di,  "ring[{i}] DI should be set for RX");
            assert!(irq, "ring[{i}] IRQ bit must be set");
        }
    }

    #[test]
    fn test_build_circular_lli_four_buffers() {
        // Four-buffer ring: verify full circular chain.
        const N: usize = 4;
        let fifo: u32 = 0x2000_AB88;
        let bases: [u32; N] = [0x1000, 0x2000, 0x3000, 0x4000];
        let segs: [(u32, u16); N] = [(bases[0], 64), (bases[1], 64), (bases[2], 64), (bases[3], 64)];
        let mut ring = [Lli::default(); N];

        build_circular_lli(
            &mut ring,
            &segs,
            fifo,
            DmaDirection::MemToPeri,
            TransferWidth::Bits32,
            BurstSize::Burst4,
        );

        for i in 0..N {
            let expected_next = &ring[(i + 1) % N] as *const Lli as u32;
            assert_eq!(
                ring[i].next_lli,
                expected_next,
                "ring[{i}].next_lli incorrect"
            );
        }
        // Last item must wrap to first (circular).
        assert_eq!(ring[N - 1].next_lli, &ring[0] as *const Lli as u32);
    }

    #[test]
    fn test_lli_control_bit_positions() {
        // Verify exact bit positions against the PAC `control.rs` field definitions.
        // TransferSize: bits [11:0]
        let ctrl = Lli::build_control(
            0xFFF, BurstSize::Burst1, BurstSize::Burst1,
            TransferWidth::Bits8, TransferWidth::Bits8, false, false, false,
        );
        assert_eq!(ctrl & 0x0FFF, 0x0FFF, "TransferSize field must occupy bits [11:0]");
        assert_eq!(ctrl >> 12, 0, "No other bits should be set");

        // SBSize: bits [13:12]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst16, BurstSize::Burst1,
            TransferWidth::Bits8, TransferWidth::Bits8, false, false, false,
        );
        assert_eq!((ctrl >> 12) & 0x3, BurstSize::Burst16 as u32, "SBSize at bits [13:12]");

        // DBSize: bits [16:15]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst1, BurstSize::Burst16,
            TransferWidth::Bits8, TransferWidth::Bits8, false, false, false,
        );
        assert_eq!((ctrl >> 15) & 0x3, BurstSize::Burst16 as u32, "DBSize at bits [16:15]");

        // SWidth: bits [19:18]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst1, BurstSize::Burst1,
            TransferWidth::Bits32, TransferWidth::Bits8, false, false, false,
        );
        assert_eq!((ctrl >> 18) & 0x3, TransferWidth::Bits32 as u32, "SWidth at bits [19:18]");

        // DWidth: bits [22:21]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst1, BurstSize::Burst1,
            TransferWidth::Bits8, TransferWidth::Bits32, false, false, false,
        );
        assert_eq!((ctrl >> 21) & 0x3, TransferWidth::Bits32 as u32, "DWidth at bits [22:21]");

        // SI: bit [26]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst1, BurstSize::Burst1,
            TransferWidth::Bits8, TransferWidth::Bits8, true, false, false,
        );
        assert_ne!(ctrl & (1 << 26), 0, "SI at bit [26]");
        assert_eq!(ctrl & (1 << 27), 0, "DI must not be set");

        // DI: bit [27]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst1, BurstSize::Burst1,
            TransferWidth::Bits8, TransferWidth::Bits8, false, true, false,
        );
        assert_ne!(ctrl & (1 << 27), 0, "DI at bit [27]");
        assert_eq!(ctrl & (1 << 26), 0, "SI must not be set");

        // I: bit [31]
        let ctrl = Lli::build_control(
            0, BurstSize::Burst1, BurstSize::Burst1,
            TransferWidth::Bits8, TransferWidth::Bits8, false, false, true,
        );
        assert_ne!(ctrl & (1 << 31), 0, "I (IRQ) at bit [31]");
    }
}
