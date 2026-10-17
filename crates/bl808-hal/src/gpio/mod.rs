use crate::glb::execute_on_glb;
use core::convert::Infallible;
use core::marker::PhantomData;
use embedded_hal::digital::{ErrorType, InputPin, OutputPin, StatefulOutputPin};

pub trait Mode {}

pub struct Input<MODE> {
    _mode: PhantomData<MODE>,
}
impl<MODE> Mode for Input<MODE> {}

pub struct Floating;
pub struct PullUp;
pub struct PullDown;

pub struct Output<MODE> {
    _mode: PhantomData<MODE>,
}
impl<MODE> Mode for Output<MODE> {}

pub struct PushPull;
pub struct OpenDrain;

pub struct Alternate<const FUNC: u8>;
impl<const FUNC: u8> Mode for Alternate<FUNC> {}

pub struct Analog;
impl Mode for Analog {}

/// Type-safe GPIO pin abstraction for the BL808.
/// P: Pin number (0-63)
/// M: Current pin mode
pub struct Pin<const P: u8, M: Mode> {
    _mode: PhantomData<M>,
}

impl<const P: u8, M: Mode> Pin<P, M> {
    pub(crate) const unsafe fn new() -> Self {
        Self { _mode: PhantomData }
    }

    /// Sets the pin to Software-controlled GPIO Push-Pull Output mode.
    pub fn into_push_pull_output(self) -> Pin<P, Output<PushPull>> {
        execute_on_glb(|glb| {
            glb.pin_cfg(P as usize).reg().modify(|_, w| {
                w.oe().set_bit()           // Output enable
                 .ie().clear_bit()         // Input disable
                 .pu().clear_bit()         // Pull-up off
                 .pd().clear_bit()         // Pull-down off
                 .func_sel().gpio();       // Function 11 (GPIO)
                unsafe { w.mode().bits(1) } // Mode 1: Set/Clear support
            });
        });
        unsafe { Pin::new() }
    }

    /// Sets the pin to Floating Input mode.
    pub fn into_floating_input(self) -> Pin<P, Input<Floating>> {
        execute_on_glb(|glb| {
            glb.pin_cfg(P as usize).reg().modify(|_, w| {
                w.oe().clear_bit()
                 .ie().set_bit()
                 .pu().clear_bit()
                 .pd().clear_bit()
                 .func_sel().gpio()
            });
        });
        unsafe { Pin::new() }
    }

    /// Sets the pin to Pull-Up Input mode.
    pub fn into_pull_up_input(self) -> Pin<P, Input<PullUp>> {
        execute_on_glb(|glb| {
            glb.pin_cfg(P as usize).reg().modify(|_, w| {
                w.oe().clear_bit()
                 .ie().set_bit()
                 .pu().set_bit()
                 .pd().clear_bit()
                 .func_sel().gpio()
            });
        });
        unsafe { Pin::new() }
    }

    /// Sets the pin to Pull-Down Input mode.
    pub fn into_pull_down_input(self) -> Pin<P, Input<PullDown>> {
        execute_on_glb(|glb| {
            glb.pin_cfg(P as usize).reg().modify(|_, w| {
                w.oe().clear_bit()
                 .ie().set_bit()
                 .pu().clear_bit()
                 .pd().set_bit()
                 .func_sel().gpio()
            });
        });
        unsafe { Pin::new() }
    }

    /// Sets the pin to an Alternate Function mode.
    pub fn into_alternate<const FUNC: u8>(self) -> Pin<P, Alternate<FUNC>> {
        execute_on_glb(|glb| {
            glb.pin_cfg(P as usize).reg().modify(|_, w| {
                unsafe { w.func_sel().bits(FUNC) }
            });
        });
        unsafe { Pin::new() }
    }
}

impl<const P: u8, M: Mode> ErrorType for Pin<P, M> {
    type Error = Infallible;
}

impl<const P: u8, MODE> OutputPin for Pin<P, Output<MODE>> {
    /// Atomic bit clear for pins 0-63.
    fn set_low(&mut self) -> Result<(), Self::Error> {
        execute_on_glb(|glb| {
            if P < 32 {
                glb.gpio_cfg140().write(|w| unsafe { w.bits(1 << P) });
            } else {
                glb.gpio_cfg141().write(|w| unsafe { w.bits(1 << (P - 32)) });
            }
        });
        Ok(())
    }

    /// Atomic bit set for pins 0-63.
    fn set_high(&mut self) -> Result<(), Self::Error> {
        execute_on_glb(|glb| {
            if P < 32 {
                glb.gpio_cfg138().write(|w| unsafe { w.bits(1 << P) });
            } else {
                glb.gpio_cfg139().write(|w| unsafe { w.bits(1 << (P - 32)) });
            }
        });
        Ok(())
    }
}

impl<const P: u8, MODE> StatefulOutputPin for Pin<P, Output<MODE>> {
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        let state = execute_on_glb(|glb| {
            if P < 32 {
                (glb.gpio_cfg128().read().bits() & (1 << P)) != 0
            } else {
                (glb.gpio_cfg129().read().bits() & (1 << (P - 32))) != 0
            }
        });
        Ok(state)
    }

    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.is_set_high()?)
    }
}

impl<const P: u8, MODE> InputPin for Pin<P, Input<MODE>> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        let state = execute_on_glb(|glb| {
            if P < 32 {
                (glb.gpio_cfg128().read().bits() & (1 << P)) != 0
            } else {
                (glb.gpio_cfg129().read().bits() & (1 << (P - 32))) != 0
            }
        });
        Ok(state)
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.is_high()?)
    }
}

/// Helper struct for all 64 pins on the BL808.
#[allow(non_snake_case)]
pub struct Pins {
    pub P0: Pin<0, Input<Floating>>,
    pub P1: Pin<1, Input<Floating>>,
    pub P2: Pin<2, Input<Floating>>,
    pub P3: Pin<3, Input<Floating>>,
    pub P4: Pin<4, Input<Floating>>,
    pub P5: Pin<5, Input<Floating>>,
    pub P6: Pin<6, Input<Floating>>,
    pub P7: Pin<7, Input<Floating>>,
    pub P8: Pin<8, Input<Floating>>,
    pub P9: Pin<9, Input<Floating>>,
    pub P10: Pin<10, Input<Floating>>,
    pub P11: Pin<11, Input<Floating>>,
    pub P12: Pin<12, Input<Floating>>,
    pub P13: Pin<13, Input<Floating>>,
    pub P14: Pin<14, Input<Floating>>,
    pub P15: Pin<15, Input<Floating>>,
    pub P16: Pin<16, Input<Floating>>,
    pub P17: Pin<17, Input<Floating>>,
    pub P18: Pin<18, Input<Floating>>,
    pub P19: Pin<19, Input<Floating>>,
    pub P20: Pin<20, Input<Floating>>,
    pub P21: Pin<21, Input<Floating>>,
    pub P22: Pin<22, Input<Floating>>,
    pub P23: Pin<23, Input<Floating>>,
    pub P24: Pin<24, Input<Floating>>,
    pub P25: Pin<25, Input<Floating>>,
    pub P26: Pin<26, Input<Floating>>,
    pub P27: Pin<27, Input<Floating>>,
    pub P28: Pin<28, Input<Floating>>,
    pub P29: Pin<29, Input<Floating>>,
    pub P30: Pin<30, Input<Floating>>,
    pub P31: Pin<31, Input<Floating>>,
    pub P32: Pin<32, Input<Floating>>,
    pub P33: Pin<33, Input<Floating>>,
    pub P34: Pin<34, Input<Floating>>,
    pub P35: Pin<35, Input<Floating>>,
    pub P36: Pin<36, Input<Floating>>,
    pub P37: Pin<37, Input<Floating>>,
    pub P38: Pin<38, Input<Floating>>,
    pub P39: Pin<39, Input<Floating>>,
    pub P40: Pin<40, Input<Floating>>,
    pub P41: Pin<41, Input<Floating>>,
    pub P42: Pin<42, Input<Floating>>,
    pub P43: Pin<43, Input<Floating>>,
    pub P44: Pin<44, Input<Floating>>,
    pub P45: Pin<45, Input<Floating>>,
}

impl Pins {
    pub(crate) fn new() -> Self {
        unsafe {
            Self {
                P0: Pin::new(), P1: Pin::new(), P2: Pin::new(), P3: Pin::new(),
                P4: Pin::new(), P5: Pin::new(), P6: Pin::new(), P7: Pin::new(),
                P8: Pin::new(), P9: Pin::new(), P10: Pin::new(), P11: Pin::new(),
                P12: Pin::new(), P13: Pin::new(), P14: Pin::new(), P15: Pin::new(),
                P16: Pin::new(), P17: Pin::new(), P18: Pin::new(), P19: Pin::new(),
                P20: Pin::new(), P21: Pin::new(), P22: Pin::new(), P23: Pin::new(),
                P24: Pin::new(), P25: Pin::new(), P26: Pin::new(), P27: Pin::new(),
                P28: Pin::new(), P29: Pin::new(), P30: Pin::new(), P31: Pin::new(),
                P32: Pin::new(), P33: Pin::new(), P34: Pin::new(), P35: Pin::new(),
                P36: Pin::new(), P37: Pin::new(), P38: Pin::new(), P39: Pin::new(),
                P40: Pin::new(), P41: Pin::new(), P42: Pin::new(), P43: Pin::new(),
                P44: Pin::new(), P45: Pin::new(),
            }
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::glb::RegisterBlock;

    /// Allocate a GLB mock big enough for all GLB registers and install it.
    fn glb_mock() -> (&'static [u32; 4096], &'static RegisterBlock) {
        let mock: &'static [u32; 4096] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 4096]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        crate::glb::install_test_mock(ptr);
        (mock, unsafe { &*ptr })
    }

    #[test]
    fn test_gpio_output_high_low() {
        let (_mock, glb) = glb_mock();

        let pins = Pins::new();
        let mut led = pins.P8.into_push_pull_output();

        // set_high drives the atomic-set register (gpio_cfg138, write-only).
        led.set_high().unwrap();
        let set_val = unsafe { core::ptr::read_volatile(glb.gpio_cfg138().as_ptr()) };
        assert_eq!(set_val, 1 << 8);

        // set_low drives the atomic-clear register (gpio_cfg140, write-only).
        led.set_low().unwrap();
        let clr_val = unsafe { core::ptr::read_volatile(glb.gpio_cfg140().as_ptr()) };
        assert_eq!(clr_val, 1 << 8);
    }

    #[test]
    fn test_gpio_input_read() {
        let (_mock, glb) = glb_mock();

        // Input level is read from gpio_cfg128 (read-only); poke the mock directly.
        unsafe { core::ptr::write_volatile(glb.gpio_cfg128().as_ptr(), 1 << 8) };

        let pins = Pins::new();
        let mut pin8 = pins.P8.into_floating_input();
        assert!(pin8.is_high().unwrap());

        unsafe { core::ptr::write_volatile(glb.gpio_cfg128().as_ptr(), 0) };
        assert!(pin8.is_low().unwrap());
    }

    #[test]
    fn test_gpio_alternate_function() {
        let (_mock, glb) = glb_mock();

        let pins = Pins::new();
        let _pin7 = pins.P7.into_alternate::<7>();

        assert_eq!(
            glb.pin_cfg(7).reg().read().func_sel().bits(),
            7,
            "func_sel for pin 7 should be alternate function 7"
        );
    }
}
