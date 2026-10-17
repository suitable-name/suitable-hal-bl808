//! Sipeed M1s Dock Board Support Definitions
//!
//! Pin mapping and convenience aliases for the Sipeed M1s Dock (Bouffalo Lab BL808).

use crate::gpio::{Alternate, Output, Pin, PushPull};

/// GPIO Pin numbers for the Sipeed M1s Dock.
pub mod pins {
    /// On-board User LED (Red, active-low). Connected to 3.3V with a pull-up resistor.
    pub const LED_RED: u8 = 8;
    /// On-board User LED (Green).
    pub const LED_GREEN: u8 = 7;
    /// On-board User LED (Blue).
    pub const LED_BLUE: u8 = 3;

    /// M0 core default console UART (UART0) TX pin (AF 7).
    pub const M0_UART_TX: u8 = 14;
    /// M0 core default console UART (UART0) RX pin (AF 7).
    pub const M0_UART_RX: u8 = 15;

    /// D0 core default console UART (UART3) TX pin.
    pub const D0_UART_TX: u8 = 16;
    /// D0 core default console UART (UART3) RX pin.
    pub const D0_UART_RX: u8 = 17;
}

/// Red LED pin configured as active-low push-pull output.
pub type LedRed = Pin<8, Output<PushPull>>;

/// Default UART0 TX pin for M0 core.
pub type M0UartTxPin = Pin<14, Alternate<7>>;

/// Default UART0 RX pin for M0 core.
pub type M0UartRxPin = Pin<15, Alternate<7>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_m1s_dock_pin_constants() {
        assert_eq!(pins::LED_RED, 8);
        assert_eq!(pins::LED_GREEN, 7);
        assert_eq!(pins::LED_BLUE, 3);
        assert_eq!(pins::M0_UART_TX, 14);
        assert_eq!(pins::M0_UART_RX, 15);
        assert_eq!(pins::D0_UART_TX, 16);
        assert_eq!(pins::D0_UART_RX, 17);
    }
}
