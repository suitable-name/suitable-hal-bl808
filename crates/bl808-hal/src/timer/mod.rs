//! Timer and Watchdog Driver
//!
//! This driver implements the Timer0 and Watchdog peripherals for the BL808 SoC.

use bl808_pac::Timer0;

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_TIMER0_PTR: core::cell::Cell<*const bl808_pac::timer0::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

pub trait TimerInstance: core::ops::Deref<Target = bl808_pac::timer0::RegisterBlock> {
    fn ptr() -> *const bl808_pac::timer0::RegisterBlock;
}

impl TimerInstance for Timer0 {
    fn ptr() -> *const bl808_pac::timer0::RegisterBlock {
        #[cfg(not(any(test, feature = "mock")))]
        return Timer0::PTR;
        #[cfg(any(test, feature = "mock"))]
        return MOCK_TIMER0_PTR.with(|p| p.get());
    }
}

#[cfg(feature = "d0")]
impl TimerInstance for bl808_pac::Timer1 {
    fn ptr() -> *const bl808_pac::timer0::RegisterBlock {
        bl808_pac::Timer1::PTR
    }
}

pub struct Timer<TIM> {
    _timer: TIM,
    clock_hz: u32,
}

impl<TIM: TimerInstance> Timer<TIM> {
    /// Creates a new `Timer` instance with a default clock frequency of 1 MHz (1 tick = 1 µs).
    pub fn new(timer: TIM) -> Self {
        Self {
            _timer: timer,
            clock_hz: 1_000_000,
        }
    }

    /// Creates a new `Timer` instance with an explicit clock frequency in Hz.
    pub fn new_with_clock(timer: TIM, clock_hz: u32) -> Self {
        Self {
            _timer: timer,
            clock_hz,
        }
    }

    /// Returns the configured clock frequency in Hz.
    pub fn clock_hz(&self) -> u32 {
        self.clock_hz
    }

    /// Sets the clock frequency in Hz.
    pub fn set_clock_hz(&mut self, clock_hz: u32) {
        self.clock_hz = clock_hz;
    }

    fn regs(&self) -> &bl808_pac::timer0::RegisterBlock {
        unsafe { &*TIM::ptr() }
    }

    /// Enables the Timer 2 counter.
    pub fn enable(&mut self) {
        self.regs().tcer().modify(|_, w| w.timer2_en().set_bit());
    }

    /// Disables the Timer 2 counter.
    pub fn disable(&mut self) {
        self.regs().tcer().modify(|_, w| w.timer2_en().clear_bit());
    }

    /// Resets the Timer 2 counter value.
    pub fn reset_counter(&mut self) {
        self.regs().tcer().modify(|_, w| w.tcr2_cnt_clr().set_bit());
    }

    /// Returns the current value of the Timer 2 counter.
    pub fn get_counter(&self) -> u32 {
        self.regs().tcr2().read().tcr2_cnt().bits()
    }

    /// Starts a periodic match on Timer 2, Match 0.
    pub fn start_periodic(&mut self, period: u32) {
        let regs = self.regs();
        // Use Timer 2, Match 0
        regs.tmr2_(0).modify(|_, w| unsafe { w.bits(period) });
        regs.tcer().modify(|_, w| w.timer2_en().set_bit());
    }

    /// Clears the Timer 2 interrupt.
    pub fn clear_interrupt(&mut self) {
        self.regs().ticr2().write(|w| w.tclr2_2().set_bit());
    }

    /// Delays for the specified number of nanoseconds using the hardware timer counter.
    pub fn delay_ns(&mut self, ns: u32) {
        if ns == 0 {
            return;
        }
        self.enable();

        let ticks = (((ns as u64) * (self.clock_hz as u64) + 999_999_999) / 1_000_000_000) as u32;
        let ticks = if ticks == 0 { 1 } else { ticks };

        let start = self.get_counter();
        #[cfg(not(any(test, feature = "mock")))]
        while self.get_counter().wrapping_sub(start) < ticks {
            core::hint::spin_loop();
        }
        #[cfg(any(test, feature = "mock"))]
        let _ = (start, ticks);
    }

    /// Delays for the specified number of microseconds using the hardware timer counter.
    pub fn delay_us(&mut self, us: u32) {
        if us == 0 {
            return;
        }
        self.enable();

        let ticks = (((us as u64) * (self.clock_hz as u64) + 999_999) / 1_000_000) as u32;
        let ticks = if ticks == 0 { 1 } else { ticks };

        let start = self.get_counter();
        #[cfg(not(any(test, feature = "mock")))]
        while self.get_counter().wrapping_sub(start) < ticks {
            core::hint::spin_loop();
        }
        #[cfg(any(test, feature = "mock"))]
        let _ = (start, ticks);
    }

    /// Delays for the specified number of milliseconds using the hardware timer counter.
    pub fn delay_ms(&mut self, ms: u32) {
        if ms == 0 {
            return;
        }
        self.enable();

        let ticks = (((ms as u64) * (self.clock_hz as u64) + 999) / 1_000) as u32;
        let ticks = if ticks == 0 { 1 } else { ticks };

        let start = self.get_counter();
        #[cfg(not(any(test, feature = "mock")))]
        while self.get_counter().wrapping_sub(start) < ticks {
            core::hint::spin_loop();
        }
        #[cfg(any(test, feature = "mock"))]
        let _ = (start, ticks);
    }
}

impl<TIM: TimerInstance> embedded_hal::delay::DelayNs for Timer<TIM> {
    fn delay_ns(&mut self, ns: u32) {
        Timer::delay_ns(self, ns);
    }

    fn delay_us(&mut self, us: u32) {
        Timer::delay_us(self, us);
    }

    fn delay_ms(&mut self, ms: u32) {
        Timer::delay_ms(self, ms);
    }
}

pub struct Watchdog<TIM> {
    _timer: TIM,
}

impl<TIM: TimerInstance> Watchdog<TIM> {
    pub fn new(timer: TIM) -> Self {
        Self { _timer: timer }
    }

    fn regs(&self) -> &bl808_pac::timer0::RegisterBlock {
        unsafe { &*TIM::ptr() }
    }

    pub fn feed(&mut self) {
        let regs = self.regs();
        // Correct unlock sequence for BL808 WDT
        regs.wfar().write(|w| unsafe { w.bits(0xBABA) });
        regs.wsar().write(|w| unsafe { w.bits(0xEB10) });
        regs.wcr().write(|w| w.wcr().set_bit());
    }

    pub fn start(&mut self, timeout: u16) {
        let regs = self.regs();
        regs.wmr().modify(|_, w| unsafe { w.wmr().bits(timeout) });
        regs.wmer().modify(|_, w| w.we().set_bit());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::timer0::RegisterBlock;

    #[test]
    fn test_timer_periodic_start() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_TIMER0_PTR.with(|p| p.set(mock_ptr));

        let tim0 = unsafe { Timer0::steal() };
        let mut timer = Timer::new(tim0);

        timer.start_periodic(5000);

        let match_val = unsafe {
            core::ptr::read_volatile(mock_ptr.cast::<u32>().add(0x10 / 4))
        };
        assert_eq!(match_val, 5000);
    }

    #[test]
    fn test_watchdog_feed_unlock() {
        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_TIMER0_PTR.with(|p| p.set(mock_ptr));

        let tim0 = unsafe { Timer0::steal() };
        let mut wdt = Watchdog::new(tim0);

        wdt.feed();

        let wfar = mock_data[0x9c / 4];
        let wsar = mock_data[0xa0 / 4];
        assert_eq!(wfar, 0xBABA);
        assert_eq!(wsar, 0xEB10);
    }

    #[test]
    fn test_timer_delay_methods() {
        use embedded_hal::delay::DelayNs;

        let mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_TIMER0_PTR.with(|p| p.set(mock_ptr));

        let tim0 = unsafe { Timer0::steal() };
        let mut timer = Timer::new(tim0);

        // Verify delay methods execute and enable the timer (bit 1 of TCER at offset 0x84)
        timer.delay_ns(100);
        let tcer_val = mock_data[0x84 / 4];
        assert_ne!(tcer_val & (1 << 1), 0, "TCER timer2_en must be enabled during delay");

        timer.delay_us(50);
        timer.delay_ms(10);

        // Test via embedded-hal DelayNs trait
        DelayNs::delay_ns(&mut timer, 500);
        DelayNs::delay_us(&mut timer, 20);
        DelayNs::delay_ms(&mut timer, 5);
    }

    #[test]
    fn test_timer_clock_and_counter() {
        let mut mock_data = [0u32; 1024];
        let mock_ptr = mock_data.as_ptr() as *const RegisterBlock;
        MOCK_TIMER0_PTR.with(|p| p.set(mock_ptr));

        let tim0 = unsafe { Timer0::steal() };
        let mut timer = Timer::new_with_clock(tim0, 2_000_000);
        assert_eq!(timer.clock_hz(), 2_000_000);

        timer.set_clock_hz(40_000_000);
        assert_eq!(timer.clock_hz(), 40_000_000);

        // Inject counter value into TCR2 (offset 0x2C)
        mock_data[0x2c / 4] = 0x1234_5678;
        assert_eq!(timer.get_counter(), 0x1234_5678);

        timer.reset_counter();
        let tcer_val = mock_data[0x84 / 4];
        assert_ne!(tcer_val & (1 << 5), 0, "TCER tcr2_cnt_clr (bit 5) must be set on reset_counter");

        timer.disable();
        let tcer_val_dis = mock_data[0x84 / 4];
        assert_eq!(tcer_val_dis & (1 << 1), 0, "TCER timer2_en should be cleared after disable");
    }
}
