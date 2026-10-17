//! ADC Driver

#[cfg(feature = "m0")]
use bl808_pac::{Aon, Gpip};

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_AON_PTR: core::cell::Cell<*const bl808_pac::aon::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
    pub static MOCK_GPIP_PTR: core::cell::Cell<*const bl808_pac::gpip::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AdcChannel {
    Ch0 = 0,
    Ch1 = 1,
    Ch2 = 2,
    Ch3 = 3,
    Ch4 = 4,
    Ch5 = 5,
    Ch6 = 6,
    Ch7 = 7,
    Ch8 = 8,
    Ch9 = 9,
    Ch10 = 10,
    Ch11 = 11,
    Gnd = 12,
}

#[cfg(feature = "m0")]
pub struct Adc {
    _aon: Aon,
    _gpip: Gpip,
}

#[cfg(feature = "m0")]
impl Adc {
    pub fn new(aon: Aon, gpip: Gpip) -> Self {
        Self { _aon: aon, _gpip: gpip }
    }

    fn aon_regs(&self) -> &bl808_pac::aon::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*Aon::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_AON_PTR.with(|p| p.get()) };
    }

    fn gpip_regs(&self) -> &bl808_pac::gpip::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*Gpip::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_GPIP_PTR.with(|p| p.get()) };
    }

    pub fn read(&mut self, channel: AdcChannel) -> u16 {
        let aon = self.aon_regs();
        let gpip = self.gpip_regs();

        aon.gpadc_reg_config2().modify(|_, w| w.gpadc_vref_sel().set_bit());

        aon.gpadc_reg_cmd().modify(|_, w| unsafe {
            w.gpadc_pos_sel().bits(channel as u8)
             .gpadc_neg_sel().bits(AdcChannel::Gnd as u8)
             .gpadc_neg_gnd().set_bit()
        });

        aon.gpadc_reg_cmd().modify(|_, w| w.gpadc_global_en().set_bit());
        aon.gpadc_reg_cmd().modify(|_, w| w.gpadc_conv_start().set_bit());

        // In test, avoid infinite loop by breaking if not configured (mock behavior)
        #[cfg(not(test))]
        while gpip.gpadc_config().read().gpadc_fifo_ne().bit_is_clear() {
            core::hint::spin_loop();
        }

        let res = gpip.gpadc_dma_rdata().read().bits();
        
        aon.gpadc_reg_cmd().modify(|_, w| {
            w.gpadc_conv_start().clear_bit()
             .gpadc_global_en().clear_bit()
        });

        (res & 0xFFF) as u16
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::{aon, gpip};

    #[test]
    fn test_adc_channel_selection() {
        let mock_aon_data = [0u32; 1024];
        let mock_gpip_data = [0u32; 1024];
        let mock_aon_ptr = mock_aon_data.as_ptr() as *const aon::RegisterBlock;
        let mock_gpip_ptr = mock_gpip_data.as_ptr() as *const gpip::RegisterBlock;

        MOCK_AON_PTR.with(|p| p.set(mock_aon_ptr));
        MOCK_GPIP_PTR.with(|p| p.set(mock_gpip_ptr));

        let aon = unsafe { Aon::steal() };
        let gpip = unsafe { Gpip::steal() };
        let mut adc = Adc::new(aon, gpip);

        adc.read(AdcChannel::Ch5);

        let cmd_val = unsafe {
            let ptr = (mock_aon_ptr as *const u32).add(0x10C / 4);
            core::ptr::read_volatile(ptr)
        };
        // POS_SEL is bits 8:12.
        let pos_sel = (cmd_val >> 8) & 0x1F;
        assert_eq!(pos_sel, 5, "POS_SEL should be set to channel 5");
    }

    #[test]
    fn test_adc_read_result() {
        let mock_aon_data = [0u32; 1024];
        let mut mock_gpip_data = [0u32; 1024];
        // Inject 0xABC as result in gpadc_dma_rdata (offset 0x04)
        mock_gpip_data[0x04 / 4] = 0xABC;

        let mock_aon_ptr = mock_aon_data.as_ptr() as *const aon::RegisterBlock;
        let mock_gpip_ptr = mock_gpip_data.as_ptr() as *const gpip::RegisterBlock;

        MOCK_AON_PTR.with(|p| p.set(mock_aon_ptr));
        MOCK_GPIP_PTR.with(|p| p.set(mock_gpip_ptr));

        let aon = unsafe { Aon::steal() };
        let gpip = unsafe { Gpip::steal() };
        let mut adc = Adc::new(aon, gpip);

        let res = adc.read(AdcChannel::Ch0);
        assert_eq!(res, 0xABC, "Should return injected ADC value");
    }

    #[test]
    fn test_adc_vref_config() {
        let mock_aon_data = [0u32; 1024];
        let mock_gpip_data = [0u32; 1024];
        let mock_aon_ptr = mock_aon_data.as_ptr() as *const aon::RegisterBlock;
        let mock_gpip_ptr = mock_gpip_data.as_ptr() as *const gpip::RegisterBlock;

        MOCK_AON_PTR.with(|p| p.set(mock_aon_ptr));
        MOCK_GPIP_PTR.with(|p| p.set(mock_gpip_ptr));

        let aon = unsafe { Aon::steal() };
        let gpip = unsafe { Gpip::steal() };
        let mut adc = Adc::new(aon, gpip);

        adc.read(AdcChannel::Ch0);

        let config2_val = unsafe {
            let ptr = (mock_aon_ptr as *const u32).add(0x114 / 4); // gpadc_reg_config2 is 0x114
            core::ptr::read_volatile(ptr)
        };
        // GPADC_VREF_SEL is bit 3 in GPADC_REG_CONFIG2
        assert_ne!(config2_val & (1 << 3), 0, "GPADC_VREF_SEL bit should be set");
    }
}
