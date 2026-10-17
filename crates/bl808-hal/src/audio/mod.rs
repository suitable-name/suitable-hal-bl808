//! Audio Subsystem Driver

use bl808_pac::Audio;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_AUDIO_PTR: core::cell::Cell<*const bl808_pac::audio::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockConfig {
    Rate48k,
    Rate44_1k,
}

pub struct AudioDriver {
    _audio: Audio,
}

impl AudioDriver {
    pub fn new(audio: Audio) -> Self {
        Self { _audio: audio }
    }

    fn regs(&self) -> &bl808_pac::audio::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*Audio::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_AUDIO_PTR.with(|p| p.get()) };
    }

    pub fn set_clock(&mut self, config: ClockConfig) {
        let regs = self.regs();
        regs.aud_top().modify(|r, w| unsafe {
            let mut val = r.bits();
            match config {
                ClockConfig::Rate48k => val &= !1,
                ClockConfig::Rate44_1k => val |= 1,
            }
            w.bits(val)
        });
    }
}

#[cfg(all(test, feature = "d0"))]
mod tests {
    use super::*;
    use bl808_pac::audio::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_AUDIO_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    fn setup_all_ones() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0xFFFF_FFFFu32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_AUDIO_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    #[test]
    fn test_audio_clock_domain_selection() {
        let regs = setup();

        let audio = unsafe { Audio::steal() };
        let mut driver = AudioDriver::new(audio);

        driver.set_clock(ClockConfig::Rate44_1k);

        let top_val = unsafe {
            let ptr = regs.aud_top().as_ptr();
            core::ptr::read_volatile(ptr)
        };
        assert_ne!(top_val & 1, 0, "Bit 0 should be set for 44.1kHz family");
    }

    #[test]
    fn test_audio_clock_48k() {
        let regs = setup_all_ones();

        let audio = unsafe { Audio::steal() };
        let mut driver = AudioDriver::new(audio);

        driver.set_clock(ClockConfig::Rate48k);

        let top_val = unsafe {
            let ptr = regs.aud_top().as_ptr();
            core::ptr::read_volatile(ptr)
        };
        assert_eq!(top_val & 1, 0, "Bit 0 should be cleared for 48kHz family");
    }

    #[test]
    fn test_audio_regs_access_safety() {
        // Setup with 0xAAAA_AAAA pattern
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0xAAAA_AAAAu32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_AUDIO_PTR.with(|p| p.set(ptr));
        let regs = unsafe { &*ptr };

        let audio = unsafe { Audio::steal() };
        let mut driver = AudioDriver::new(audio);

        driver.set_clock(ClockConfig::Rate44_1k);

        let top_val = unsafe {
            let ptr = regs.aud_top().as_ptr();
            core::ptr::read_volatile(ptr)
        };
        assert_eq!(top_val, 0xAAAA_AAAB, "Should preserve other bits while setting bit 0");
    }
}
