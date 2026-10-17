//! Pulse Width Modulation (PWM) Driver
//!
//! This driver implements the PWM peripheral for the BL808 SoC.
//! It provides a group-based abstraction for managing multiple PWM channels.

use bl808_pac::Pwm;
use core::marker::PhantomData;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_PWM_PTR: core::cell::Cell<*const bl808_pac::pwm::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait PwmInstance: core::ops::Deref<Target = bl808_pac::pwm::RegisterBlock> {
    fn ptr() -> *const bl808_pac::pwm::RegisterBlock;
}

impl PwmInstance for Pwm {
    fn ptr() -> *const bl808_pac::pwm::RegisterBlock {
        #[cfg(not(test))]
        return Pwm::PTR;
        #[cfg(test)]
        return MOCK_PWM_PTR.with(|p| p.get());
    }
}

pub struct PwmDriver<PWM> {
    _pwm: PWM,
}

impl<PWM: PwmInstance> PwmDriver<PWM> {
    pub fn new(pwm: PWM) -> Self {
        Self { _pwm: pwm }
    }

    pub fn split(self) -> (PwmGroup<PWM, 0>, PwmGroup<PWM, 1>) {
        (
            PwmGroup { _marker: PhantomData },
            PwmGroup { _marker: PhantomData },
        )
    }
}

pub struct PwmGroup<PWM, const G: u8> {
    _marker: PhantomData<PWM>,
}

impl<PWM: PwmInstance, const G: u8> PwmGroup<PWM, G> {
    fn regs(&self) -> &bl808_pac::pwm::Grp {
        let pwm = unsafe { &*PWM::ptr() };
        pwm.grp(G as usize)
    }

    pub fn set_period(&mut self, period: u32) {
        self.regs().period().modify(|_, w| unsafe { w.pwm_period().bits(period as u16) });
    }

    pub fn set_duty(&mut self, channel: usize, duty: u32) {
        if channel < 4 {
            self.regs().thre(channel).modify(|_, w| unsafe {
                w.thre_l().bits(0).thre_h().bits(duty as u16)
            });
        }
    }

    pub fn enable(&mut self) {
        self.regs().config0().modify(|_, w| w.pwm_stop_en().clear_bit());
    }

    pub fn disable(&mut self) {
        self.regs().config0().modify(|_, w| w.pwm_stop_en().set_bit());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::pwm::RegisterBlock;

    #[test]
    fn test_pwm_period_config() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_PWM_PTR.with(|p| p.set(mock_ptr));

        let pwm = unsafe { Pwm::steal() };
        let driver = PwmDriver::new(pwm);
        let (mut grp0, _) = driver.split();

        grp0.set_period(1000);

        let val = unsafe {
            // Offset 0x08 is period for Grp 0
            core::ptr::read_volatile(mock_ptr.cast::<u32>().add(2))
        };
        assert_eq!(val, 1000);
    }
}
