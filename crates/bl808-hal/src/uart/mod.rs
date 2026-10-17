//! UART Driver
//!
//! This driver implements the UART peripheral for the BL808 SoC.
//! It provides both a raw interface and implementations of the `embedded-hal` traits.

use crate::gpio::{Alternate, Pin};
use bl808_pac::Uart0;
use embedded_hal_nb::serial::{ErrorKind, ErrorType, Read, Write};
use nb;

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_UART0_PTR: core::cell::Cell<*const bl808_pac::uart0::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(all(feature = "d0", any(test, feature = "mock")))]
std::thread_local! {
    pub static MOCK_UART3_PTR: core::cell::Cell<*const bl808_pac::uart0::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait UartInstance: core::ops::Deref<Target = bl808_pac::uart0::RegisterBlock> {
    fn ptr() -> *const bl808_pac::uart0::RegisterBlock;
}

impl UartInstance for Uart0 {
    fn ptr() -> *const bl808_pac::uart0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return Uart0::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_UART0_PTR.with(|p| p.get());
    }
}

#[cfg(any(feature = "m0", feature = "lp"))]
impl UartInstance for bl808_pac::Uart1 {
    fn ptr() -> *const bl808_pac::uart0::RegisterBlock {
        bl808_pac::Uart1::PTR
    }
}

#[cfg(any(feature = "m0", feature = "lp"))]
impl UartInstance for bl808_pac::Uart2 {
    fn ptr() -> *const bl808_pac::uart0::RegisterBlock {
        bl808_pac::Uart2::PTR
    }
}

#[cfg(feature = "d0")]
impl UartInstance for bl808_pac::Uart3 {
    fn ptr() -> *const bl808_pac::uart0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return bl808_pac::Uart3::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_UART3_PTR.with(|p| p.get());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    Framing,
    Parity,
    Overrun,
}

impl embedded_hal_nb::serial::Error for SerialError {
    fn kind(&self) -> ErrorKind {
        match self {
            SerialError::Framing => ErrorKind::FrameFormat,
            SerialError::Parity => ErrorKind::Parity,
            SerialError::Overrun => ErrorKind::Overrun,
        }
    }
}

pub struct Serial<UART, TX, RX> {
    _uart: UART,
    _tx: TX,
    _rx: RX,
}

impl<UART, TX, RX> Serial<UART, TX, RX>
where
    UART: UartInstance,
{
    pub fn new<const P_TX: u8, const P_RX: u8, const AF: u8>(
        uart: UART,
        tx: TX,
        rx: RX,
        baudrate: u32,
        bclk_freq: u32,
    ) -> Serial<UART, Pin<P_TX, Alternate<AF>>, Pin<P_RX, Alternate<AF>>>
    where
        TX: Into<Pin<P_TX, Alternate<AF>>>,
        RX: Into<Pin<P_RX, Alternate<AF>>>,
    {
        let regs = unsafe { &*UART::ptr() };

        // baudrate period = bclk / baudrate - 1
        let divisor = bclk_freq / baudrate;
        let divisor_u16 = (divisor - 1) as u16;
        let period_val = (divisor_u16 as u32) | ((divisor_u16 as u32) << 16);
        regs.uart_bit_prd().write(|w| unsafe { w.bits(period_val) });

        // Enable TX/RX
        regs.utx_config().modify(|_, w| w.cr_utx_en().set_bit());
        regs.urx_config().modify(|_, w| w.cr_urx_en().set_bit());

        Serial {
            _uart: uart,
            _tx: tx.into(),
            _rx: rx.into(),
        }
    }

    fn regs(&self) -> &bl808_pac::uart0::RegisterBlock {
        unsafe { &*UART::ptr() }
    }

    /// Writes a single byte to the UART TX FIFO, spinning until space is available.
    pub fn write_byte(&mut self, byte: u8) {
        #[cfg(not(any(test, feature = "mock")))]
        while self.regs().uart_fifo_config_1().read().tx_fifo_cnt().bits() == 0 {
            core::hint::spin_loop();
        }
        self.regs().uart_fifo_wdata().write(|w| unsafe { w.bits(byte as u32) });
    }

    /// Writes a slice of bytes to the UART TX FIFO, spinning on each byte until space is available.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_byte(b);
        }
    }

    /// Blocks until any pending TX transmission has finished.
    pub fn flush_tx(&mut self) {
        #[cfg(not(any(test, feature = "mock")))]
        while self.regs().uart_status().read().sts_utx_bus_busy().bit_is_set() {
            core::hint::spin_loop();
        }
    }
}

impl<UART: UartInstance, TX, RX> core::fmt::Write for Serial<UART, TX, RX> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

impl<UART: UartInstance, TX, RX> ErrorType for Serial<UART, TX, RX> {
    type Error = SerialError;
}

impl<UART: UartInstance, TX, RX> Write for Serial<UART, TX, RX> {
    fn write(&mut self, word: u8) -> nb::Result<(), Self::Error> {
        let regs = self.regs();

        // tx_fifo_cnt is free slots; 0 = full.
        if regs.uart_fifo_config_1().read().tx_fifo_cnt().bits() == 0 {
            return Err(nb::Error::WouldBlock);
        }

        regs.uart_fifo_wdata().write(|w| unsafe { w.bits(word as u32) });
        Ok(())
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        let regs = self.regs();
        if regs.uart_status().read().sts_utx_bus_busy().bit_is_set() {
            Err(nb::Error::WouldBlock)
        } else {
            Ok(())
        }
    }
}

impl<UART: UartInstance, TX, RX> Read for Serial<UART, TX, RX> {
    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        let regs = self.regs();
        let sts = regs.uart_int_sts().read();

        if sts.urx_fer_int().bit_is_set() {
            // The RX FIFO overflow/underflow interrupt (urx_fer_int) has no
            // dedicated clear bit in UART_INT_CLEAR (confirmed against PAC and
            // SDK bl808_uart.c `UART_IntHandler`: the UART_INT_RX_FER case does
            // NOT call BL_WR_REG(UART_INT_CLEAR)).  The interrupt is cleared by
            // flushing the RX FIFO via UART_FIFO_CONFIG_0.rx_fifo_clr (bit 3),
            // matching SDK `UART_RxFifoClear` (bl808_uart.c line 949-962).
            regs.uart_fifo_config_0()
                .modify(|_, w| w.rx_fifo_clr().set_bit());
            return Err(nb::Error::Other(SerialError::Framing));
        }

        if regs.uart_fifo_config_1().read().rx_fifo_cnt().bits() == 0 {
            return Err(nb::Error::WouldBlock);
        }

        Ok(regs.uart_fifo_rdata().read().bits() as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::uart0::RegisterBlock;
    use crate::gpio::Pins;

    #[test]
    fn test_uart_baudrate_calculation() {
        let mock_data = [0u32; 256];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_UART0_PTR.with(|p| p.set(mock_ptr));

        // Pin alternate-function setup routes through GLB; install a GLB mock
        // for this thread so `into_alternate` does not dereference a null ptr.
        let glb_mock = [0u32; 4096];
        crate::glb::install_test_mock(glb_mock.as_ptr() as *const bl808_pac::glb::RegisterBlock);

        let uart0 = unsafe { Uart0::steal() };
        let pins = Pins::new();
        let tx = pins.P14.into_alternate::<7>();
        let rx = pins.P15.into_alternate::<7>();

        let _serial = Serial::new(uart0, tx, rx, 115200, 40_000_000);

        let bit_prd_val = unsafe {
            let ptr = (mock_ptr as *const u32).add(0x08 / 4);
            core::ptr::read_volatile(ptr)
        };
        assert_eq!(bit_prd_val, 0x015A015A);
    }

    /// Verify that when `urx_fer_int` is set the `Read` impl:
    ///   1. Returns `Err(nb::Error::Other(SerialError::Framing))`
    ///   2. Writes `rx_fifo_clr` (bit 3 of UART_FIFO_CONFIG_0) to clear the error.
    #[test]
    fn test_read_rx_fer_clears_fifo() {
        let mock: &'static mut [u32; 256] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 256]));
        let mock_ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_UART0_PTR.with(|p| p.set(mock_ptr));

        let glb_mock: &'static mut [u32; 4096] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 4096]));
        crate::glb::install_test_mock(
            glb_mock.as_ptr() as *const bl808_pac::glb::RegisterBlock,
        );

        let uart0 = unsafe { Uart0::steal() };
        let pins = Pins::new();
        let tx = pins.P14.into_alternate::<7>();
        let rx = pins.P15.into_alternate::<7>();
        let mut serial = Serial::new(uart0, tx, rx, 115200, 40_000_000);

        // UART_INT_STS is at offset 0x20.  Set urx_fer_int = bit 7.
        // UART_FIFO_CONFIG_1 (rx_fifo_cnt) is at offset 0x84; leave cnt = 0.
        unsafe {
            let sts_ptr = (mock_ptr as *mut u32).add(0x20 / 4);
            core::ptr::write_volatile(sts_ptr, 1 << 7);
        }

        let result = Read::read(&mut serial);
        assert!(
            matches!(result, Err(nb::Error::Other(SerialError::Framing))),
            "expected Framing error, got {:?}",
            result
        );

        // rx_fifo_clr is bit 3 of UART_FIFO_CONFIG_0 at offset 0x80.
        let fifo_cfg0 = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x80 / 4))
        };
        assert_ne!(fifo_cfg0 & (1 << 3), 0, "rx_fifo_clr (bit 3) must be set to acknowledge urx_fer_int");
    }

    #[test]
    fn test_serial_fmt_write() {
        use core::fmt::Write;

        let mock: &'static mut [u32; 256] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 256]));
        let mock_ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_UART0_PTR.with(|p| p.set(mock_ptr));

        let glb_mock: &'static mut [u32; 4096] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 4096]));
        crate::glb::install_test_mock(
            glb_mock.as_ptr() as *const bl808_pac::glb::RegisterBlock,
        );

        let uart0 = unsafe { Uart0::steal() };
        let pins = Pins::new();
        let tx = pins.P14.into_alternate::<7>();
        let rx = pins.P15.into_alternate::<7>();
        let mut serial = Serial::new(uart0, tx, rx, 115200, 40_000_000);

        // Test writing formatted string
        write!(serial, "BL808 val: {}\n", 42).expect("fmt write must succeed");

        // The last character written to uart_fifo_wdata (offset 0x88) should be '\n' (0x0A)
        let last_wdata = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x88 / 4))
        };
        assert_eq!(last_wdata, b'\n' as u32);
    }

    #[test]
    fn test_serial_write_bytes() {
        let mock: &'static mut [u32; 256] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 256]));
        let mock_ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_UART0_PTR.with(|p| p.set(mock_ptr));

        let glb_mock: &'static mut [u32; 4096] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 4096]));
        crate::glb::install_test_mock(
            glb_mock.as_ptr() as *const bl808_pac::glb::RegisterBlock,
        );

        let uart0 = unsafe { Uart0::steal() };
        let pins = Pins::new();
        let tx = pins.P14.into_alternate::<7>();
        let rx = pins.P15.into_alternate::<7>();
        let mut serial = Serial::new(uart0, tx, rx, 115200, 40_000_000);

        serial.write_bytes(b"OK");
        let last_wdata = unsafe {
            core::ptr::read_volatile((mock_ptr as *const u32).add(0x88 / 4))
        };
        assert_eq!(last_wdata, b'K' as u32);

        serial.flush_tx();
    }
}
