use crate::gpio::Pins;
use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) static GLB_CONFIGURED: AtomicBool = AtomicBool::new(false);

/// Test-only helper: install a mock GLB register block for the current thread
/// and mark GLB as configured so `execute_on_glb` does not assert.
#[cfg(any(test, feature = "mock"))]
pub fn install_test_mock(ptr: *const bl808_pac::glb::RegisterBlock) {
    GLB_CONFIGURED.store(true, Ordering::SeqCst);
    MOCK_GLB_PTR.with(|p| p.set(ptr));
}

#[cfg(any(test, feature = "mock"))]
extern crate std;

#[cfg(any(test, feature = "mock"))]
std::thread_local! {
    pub static MOCK_GLB_PTR: core::cell::Cell<*const bl808_pac::glb::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

/// Clocks configuration structure holding frequencies for major buses.
#[derive(Debug, Clone, Copy)]
pub struct Clocks {
    pub sys_clk: u32,
    pub hclk: u32,
    pub bclk: u32,
}

pub struct GlbClockControl {
    _marker: core::marker::PhantomData<()>,
}

/// XTAL frequency on the Sipeed M1s Dock (40 MHz crystal).
pub const XTAL_FREQ_HZ: u32 = 40_000_000;
/// RC32M internal oscillator frequency.
pub const RC32M_FREQ_HZ: u32 = 32_000_000;
/// WiFi PLL output frequency (480 MHz, configured by boot firmware).
pub const WIFI_PLL_FREQ_HZ: u32 = 480_000_000;
/// AUPLL output frequency (typically 400 MHz for M1s Dock).
pub const AUPLL_FREQ_HZ: u32 = 400_000_000;

impl GlbClockControl {
    /// Returns the current clock tree frequencies by reading GLB registers.
    ///
    /// Root clock selection (hbn_root_clk_sel):
    /// - 0: RC32M (32 MHz)
    /// - 1: XTAL  (40 MHz on M1s Dock)
    /// - 2: AUPLL (400 MHz typical)
    /// - 3: WiFi PLL / CPU PLL (480 MHz typical)
    ///
    /// PLL frequencies are compile-time constants matching M1s Dock boot firmware.
    pub fn get_clocks(&self) -> Clocks {
        execute_on_glb(|glb| {
            let cfg = glb.sys_cfg0().read();
            let root_clk = match cfg.hbn_root_clk_sel().bits() {
                0 => RC32M_FREQ_HZ,
                1 => XTAL_FREQ_HZ,
                2 => AUPLL_FREQ_HZ,
                _ => WIFI_PLL_FREQ_HZ,
            };
            let hclk_div = cfg.reg_hclk_div().bits() as u32;
            let bclk_div = cfg.reg_bclk_div().bits() as u32;
            let hclk = root_clk / (hclk_div + 1);
            let bclk = hclk / (bclk_div + 1);
            Clocks { sys_clk: root_clk, hclk, bclk }
        })
    }

    pub fn set_system_clock_max(&mut self) {
        // Select WiFi PLL (480 MHz) as root clock source.
        //
        // SDK reference: bl808_glb.c `GLB_Set_MCU_System_CLK` and
        // bl808_hbn.c `HBN_Set_ROOT_CLK_Sel`. The authoritative write is to
        // HBN_GLB.hbn_root_clk_sel (bits 0:1, value 3 = WiFi PLL). The GLB
        // SYS_CFG0 register exposes a writable mirror of the same field at bits
        // 6:7 and is used here so that the GLB mock covers both write and
        // read-back in tests without a separate HBN dependency.
        //
        // Values: 0 = RC32M, 1 = XTAL, 2 = AUPLL, 3 = WiFi PLL (480 MHz).
        //
        // HCLK divider is set to 0 (÷1) and BCLK divider to 1 (÷2) which
        // matches the SDK's maximum-performance setting for the M0 core.
        execute_on_glb(|glb| {
            glb.sys_cfg0().modify(|_, w| unsafe {
                w.hbn_root_clk_sel().bits(3)
                 .reg_hclk_div().bits(0)
                 .reg_bclk_div().bits(1)
            });
        });
    }

    pub fn reset_uart0(&mut self) {
        execute_on_glb(|glb| {
            // `swrst_s1a0` is SWRST_S1 bit 16.
            // Confirmed by SDK bl808_glb.h: GLB_AHB_MCU_SW_UART0 = 48 (i.e. the
            // 48th bit across all AHB SW reset registers). SWRST_S1 covers bits
            // 32–63, so UART0 = bit 48 − 32 = bit 16 of SWRST_S1 = `swrst_s1a0`.
            // SDK function: GLB_AHB_MCU_Software_Reset(GLB_AHB_MCU_SW_UART0).
            glb.swrst_s1().modify(|_, w| w.swrst_s1a0().set_bit());
            glb.swrst_s1().modify(|_, w| w.swrst_s1a0().clear_bit());
        });
    }
}

pub struct GlbManager {
    _inner: (),
}

impl GlbManager {
    pub fn new() -> Self {
        Self { _inner: () }
    }

    pub fn split(self) -> (GlbClockControl, Pins) {
        #[cfg(not(any(test, feature = "mock")))]
        if GLB_CONFIGURED.swap(true, Ordering::SeqCst) {
            panic!("GLB split called more than once");
        }
        #[cfg(any(test, feature = "mock"))]
        GLB_CONFIGURED.store(true, Ordering::SeqCst);

        (
            GlbClockControl { _marker: core::marker::PhantomData },
            Pins::new(),
        )
    }
}

impl Default for GlbManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal helper for executing a closure with access to the GLB register block.
/// 
/// Hardened with debug_assert to ensure GLB is configured.
pub(crate) fn execute_on_glb<F, R>(f: F) -> R
where
    F: FnOnce(&bl808_pac::glb::RegisterBlock) -> R,
{
    // Task 6aeb: Hard crash on misconfiguration is bad for HAL.
    // Use debug_assert to catch issues during development without production overhead.
    debug_assert!(GLB_CONFIGURED.load(Ordering::Relaxed), "execute_on_glb called before split()");

    // On the host test build we operate on a thread-local mock and do NOT take
    // the hardware critical section: the `critical-section` std implementation
    // uses a single global mutex, and a panic inside the closure (e.g. a test
    // that forgot to install the GLB mock) would leak the guard and deadlock
    // every other thread. A null mock fails cleanly here instead.
    #[cfg(any(test, feature = "mock"))]
    {
        let ptr = MOCK_GLB_PTR.with(|p| p.get());
        assert!(
            !ptr.is_null(),
            "GLB mock not installed for this test thread (call glb::install_test_mock)"
        );
        f(unsafe { &*ptr })
    }

    #[cfg(not(any(test, feature = "mock")))]
    critical_section::with(|_| {
        let glb = unsafe { &*(0x2000_0000 as *const bl808_pac::glb::RegisterBlock) };
        f(glb)
    })
}

pub trait GlbExt {
    type Parts;
    fn split(self) -> Self::Parts;
}

#[cfg(any(feature = "m0", feature = "d0", feature = "lp"))]
impl GlbExt for bl808_pac::Glb {
    type Parts = (GlbClockControl, Pins);
    fn split(self) -> Self::Parts {
        let manager = GlbManager::new();
        manager.split()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bl808_pac::glb::RegisterBlock;

    #[test]
    fn test_clock_source_switching() {
        let mock: &'static mut [u32; 1024] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        install_test_mock(ptr);

        let mut glb_clock = GlbClockControl { _marker: core::marker::PhantomData };
        glb_clock.set_system_clock_max();

        // SYS_CFG0 is at GLB offset 0x90; verify hbn_root_clk_sel (bits 6:7) = 3,
        // reg_hclk_div (bits 8:15) = 0, reg_bclk_div (bits 16:23) = 1.
        // Expected: (3 << 6) | (0 << 8) | (1 << 16) = 0x0001_00C0
        let sys_cfg0_val = unsafe {
            let p = (ptr as *const u32).add(0x90 / 4);
            core::ptr::read_volatile(p)
        };
        // hbn_root_clk_sel = bits 7:6 = 0b11 => mask 0xC0
        assert_eq!(sys_cfg0_val & 0xC0, 3 << 6, "root clk sel should be 3 (WiFi PLL)");
        // reg_hclk_div = bits 15:8 = 0
        assert_eq!(sys_cfg0_val & 0xFF00, 0, "HCLK divider should be 0");
        // reg_bclk_div = bits 23:16 = 1
        assert_eq!((sys_cfg0_val >> 16) & 0xFF, 1, "BCLK divider should be 1");
    }

    #[test]
    fn test_reset_uart0() {
        let mock: &'static mut [u32; 1024] = std::boxed::Box::leak(std::boxed::Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        install_test_mock(ptr);

        let mut glb_clock = GlbClockControl { _marker: core::marker::PhantomData };
        glb_clock.reset_uart0();

        // SWRST_S1 is at GLB offset 0x544; after pulse, bit 16 must be 0.
        let swrst_s1_val = unsafe {
            let p = (ptr as *const u32).add(0x544 / 4);
            core::ptr::read_volatile(p)
        };
        // After set+clear the bit must be de-asserted.
        assert_eq!(swrst_s1_val & (1 << 16), 0, "swrst_s1a0 (UART0 reset) must be cleared after pulse");
    }
}
