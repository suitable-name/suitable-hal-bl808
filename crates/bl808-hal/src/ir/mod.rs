//! Infrared (IR) Driver
//!
//! Supports both TX and RX, plus hardware-assisted NEC protocol encoding and
//! a pure-Rust NEC frame encoder/decoder.
//!
//! # NEC protocol overview
//!
//! - Carrier:    38 kHz
//! - Leader:     9 ms burst + 4.5 ms space
//! - Bit '0':    560 µs burst + 560 µs space
//! - Bit '1':    560 µs burst + 1690 µs space
//! - Stop bit:   560 µs burst
//! - Frame:      32 bits LSB-first — `addr | !addr<<8 | cmd<<16 | !cmd<<24`

use bl808_pac::Ir;
use core::marker::PhantomData;

// Re-export the portable NEC helpers so callers can access them via either
// `bl808_hal::ir::{nec_frame, decode_nec}` or `bl808_hal::nec::…`.
pub use crate::nec::{decode_nec, nec_frame};

#[cfg(test)]
extern crate std;

#[cfg(all(test, feature = "m0"))]
std::thread_local! {
    pub static MOCK_IR_PTR: core::cell::Cell<*const bl808_pac::ir::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

// ---------------------------------------------------------------------------
// IrInstance trait
// ---------------------------------------------------------------------------

pub trait IrInstance: core::ops::Deref<Target = bl808_pac::ir::RegisterBlock> {
    fn ptr() -> *const bl808_pac::ir::RegisterBlock;
}

impl IrInstance for Ir {
    fn ptr() -> *const bl808_pac::ir::RegisterBlock {
        #[cfg(not(test))]
        return Ir::PTR;
        #[cfg(test)]
        return MOCK_IR_PTR.with(|p| p.get());
    }
}

// ---------------------------------------------------------------------------
// IrDriver
// ---------------------------------------------------------------------------

/// Owns the IR peripheral token and splits it into TX and RX halves.
pub struct IrDriver<IR> {
    _ir: IR,
}

impl<IR: IrInstance> IrDriver<IR> {
    pub fn new(ir: IR) -> Self {
        Self { _ir: ir }
    }

    /// Split into separate TX and RX handles.
    pub fn split(self) -> (IrTx<IR>, IrRx<IR>) {
        (
            IrTx { _marker: PhantomData },
            IrRx { _marker: PhantomData },
        )
    }
}

// ---------------------------------------------------------------------------
// IrTx
// ---------------------------------------------------------------------------

/// IR transmit half.
pub struct IrTx<IR> {
    _marker: PhantomData<IR>,
}

impl<IR: IrInstance> IrTx<IR> {
    fn regs(&self) -> &bl808_pac::ir::RegisterBlock {
        unsafe { &*IR::ptr() }
    }

    /// Enable the IRTX module.
    pub fn enable(&mut self) {
        self.regs()
            .irtx_config()
            .modify(|_, w| w.cr_irtx_en().set_bit());
    }

    /// Disable the IRTX module.
    pub fn disable(&mut self) {
        self.regs()
            .irtx_config()
            .modify(|_, w| w.cr_irtx_en().clear_bit());
    }

    /// Write one word into the TX FIFO.
    pub fn transmit(&mut self, data: u32) {
        self.regs()
            .ir_fifo_wdata()
            .write(|w| unsafe { w.bits(data) });
    }

    /// Configure hardware registers for NEC protocol transmission.
    ///
    /// `clk_hz` — system clock in Hz used to derive the pulse-width time base.
    ///
    /// A time unit T ≈ 100 µs is selected by setting `pw_unit = clk_hz / 10_000`
    /// (capped at the 12-bit field maximum of 4095). All NEC burst/space widths
    /// are expressed as multiples of T.
    ///
    /// | Pulse          | Duration  | T units (T=100 µs) |
    /// |----------------|-----------|---------------------|
    /// | Leader burst   | 9 000 µs  | 90                  |
    /// | Leader space   | 4 500 µs  | 45                  |
    /// | Bit mark       |   560 µs  |  6                  |
    /// | Bit '0' space  |   560 µs  |  6                  |
    /// | Bit '1' space  | 1 690 µs  | 17                  |
    /// | Stop mark      |   560 µs  |  6                  |
    pub fn configure_nec(&mut self, clk_hz: u32) {
        let pw_unit = (clk_hz / 10_000).min(0x0FFF) as u16;
        let regs = self.regs();

        // Pulse-width time base + carrier modulation phases (38 kHz defaults).
        regs.irtx_pulse_width().modify(|_, w| unsafe {
            w.cr_irtx_pw_unit()
                .bits(pw_unit)
                .cr_irtx_mod_ph0_w()
                .bits(0x11) // carrier high phase
                .cr_irtx_mod_ph1_w()
                .bits(0x22) // carrier low phase
        });

        // Bit '0': 560 µs + 560 µs.  Bit '1': 560 µs + 1690 µs.
        regs.irtx_pw_0().modify(|_, w| unsafe {
            w.cr_irtx_logic0_ph0_w()
                .bits(6)
                .cr_irtx_logic0_ph1_w()
                .bits(6)
                .cr_irtx_logic1_ph0_w()
                .bits(6)
                .cr_irtx_logic1_ph1_w()
                .bits(17)
        });

        // Leader: 9 ms burst + 4.5 ms space.  Stop: 560 µs burst.
        regs.irtx_pw_1().modify(|_, w| unsafe {
            w.cr_irtx_head_ph0_w()
                .bits(90)
                .cr_irtx_head_ph1_w()
                .bits(45)
                .cr_irtx_tail_ph0_w()
                .bits(6)
                .cr_irtx_tail_ph1_w()
                .bits(0)
        });

        // Enable modulation, data, head, tail, frame mode; 32 data bits (=31).
        regs.irtx_config().modify(|_, w| unsafe {
            w.cr_irtx_mod_en()
                .set_bit()
                .cr_irtx_data_en()
                .set_bit()
                .cr_irtx_head_en()
                .set_bit()
                .cr_irtx_tail_en()
                .set_bit()
                .cr_irtx_frm_en()
                .set_bit()
                .cr_irtx_data_num()
                .bits(31) // 0-indexed: 31 = 32 bits
        });
    }

    /// Encode an NEC `(addr, cmd)` pair and push the frame into the TX FIFO.
    ///
    /// Call [`configure_nec`] and [`enable`] before calling this method.
    pub fn transmit_nec(&mut self, addr: u8, cmd: u8) {
        self.transmit(nec_frame(addr, cmd));
    }
}

// ---------------------------------------------------------------------------
// IrRx
// ---------------------------------------------------------------------------

/// IR receive half.
pub struct IrRx<IR> {
    _marker: PhantomData<IR>,
}

impl<IR: IrInstance> IrRx<IR> {
    fn regs(&self) -> &bl808_pac::ir::RegisterBlock {
        unsafe { &*IR::ptr() }
    }

    /// Enable the IRRX module.
    pub fn enable(&mut self) {
        self.regs()
            .irrx_config()
            .modify(|_, w| w.cr_irrx_en().set_bit());
    }

    /// Disable the IRRX module.
    pub fn disable(&mut self) {
        self.regs()
            .irrx_config()
            .modify(|_, w| w.cr_irrx_en().clear_bit());
    }

    /// Read a raw 32-bit word from the RX FIFO.
    pub fn receive(&self) -> u32 {
        self.regs().ir_fifo_rdata().read().bits()
    }

    /// Read the received 32-bit frame from `irrx_data_word0` and decode it
    /// as an NEC frame.
    ///
    /// Returns `Some((addr, cmd))` on success, `None` on checksum failure.
    pub fn receive_nec(&self) -> Option<(u8, u8)> {
        let frame = self.regs().irrx_data_word0().read().bits();
        decode_nec(frame)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // Hardware-register tests — only compile with m0 feature.
    #[cfg(feature = "m0")]
    mod hw {
        use super::super::*;
        use bl808_pac::ir::RegisterBlock;

        #[test]
        fn test_ir_tx_enable() {
            let mock_data = [0u32; 256];
            let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
            MOCK_IR_PTR.with(|p| p.set(mock_ptr));

            let ir = unsafe { Ir::steal() };
            let driver = IrDriver::new(ir);
            let (mut tx, _) = driver.split();
            tx.enable();

            // irtx_config at offset 0x00 → word 0; bit 0 = cr_irtx_en
            let val = unsafe { core::ptr::read_volatile(mock_ptr as *const u32) };
            assert_ne!(val & 1, 0, "cr_irtx_en (bit 0) must be set");
        }

        #[test]
        fn test_ir_rx_enable() {
            let mock_data = [0u32; 256];
            let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
            MOCK_IR_PTR.with(|p| p.set(mock_ptr));

            let ir = unsafe { Ir::steal() };
            let driver = IrDriver::new(ir);
            let (_, mut rx) = driver.split();
            rx.enable();

            // irrx_config at offset 0x40 → word 16; bit 0 = cr_irrx_en
            let val = unsafe {
                core::ptr::read_volatile((mock_ptr as *const u32).add(0x40 / 4))
            };
            assert_ne!(val & 1, 0, "cr_irrx_en (bit 0) must be set");
        }

        #[test]
        fn test_configure_nec_pw_unit() {
            let mock_data = [0u32; 256];
            let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
            MOCK_IR_PTR.with(|p| p.set(mock_ptr));

            let ir = unsafe { Ir::steal() };
            let driver = IrDriver::new(ir);
            let (mut tx, _) = driver.split();
            // 80 MHz clock → pw_unit = 80_000_000 / 10_000 = 8000, capped to 4095
            tx.configure_nec(80_000_000);

            // irtx_pulse_width at offset 0x10 → word 4; bits 0:11 = pw_unit
            let val = unsafe {
                core::ptr::read_volatile((mock_ptr as *const u32).add(0x10 / 4))
            };
            assert_eq!(val & 0x0FFF, 4095, "pw_unit should be capped at 4095");
        }

        #[test]
        fn test_configure_nec_pw_unit_40mhz() {
            let mock_data = [0u32; 256];
            let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
            MOCK_IR_PTR.with(|p| p.set(mock_ptr));

            let ir = unsafe { Ir::steal() };
            let driver = IrDriver::new(ir);
            let (mut tx, _) = driver.split();
            // 40 MHz → pw_unit = 40_000_000 / 10_000 = 4000 (fits in 12 bits)
            tx.configure_nec(40_000_000);

            let val = unsafe {
                core::ptr::read_volatile((mock_ptr as *const u32).add(0x10 / 4))
            };
            assert_eq!(val & 0x0FFF, 4000, "pw_unit should be 4000 at 40 MHz");
        }

        #[test]
        fn test_transmit_nec_writes_fifo() {
            let mock_data = [0u32; 256];
            let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
            MOCK_IR_PTR.with(|p| p.set(mock_ptr));

            let ir = unsafe { Ir::steal() };
            let driver = IrDriver::new(ir);
            let (mut tx, _) = driver.split();
            tx.transmit_nec(0x59, 0xA3);

            // ir_fifo_wdata at offset 0x88 → word 34
            let val = unsafe {
                core::ptr::read_volatile((mock_ptr as *const u32).add(0x88 / 4))
            };
            assert_eq!(val, nec_frame(0x59, 0xA3));
        }
    }
}
