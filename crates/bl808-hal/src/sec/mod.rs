//! Security Engine (SEC) Driver
//!
//! Uses real PAC registers from `bl808_pac::sec_eng`:
//!
//! - `load_aes_key`   → SE_AES_0_KEY_0..3: se_aes_0_key field (words 0-3 of a 128-bit key)
//! - `trigger_sha256` → SE_SHA_0_CTRL: se_sha_0_en=1, se_sha_0_mode=0 (SHA-256),
//!   se_sha_0_trig_1t=1 (pulse)
//! - `is_busy`        → SE_SHA_0_CTRL: se_sha_0_busy (bit 0)
//!
//! Note: `is_busy` covers the SHA engine only. The AES engine has its own
//! busy bit (`se_aes_0_busy` in SE_AES_0_CTRL) which is separate.

#[cfg(feature = "m0")]
use bl808_pac::SecEng as PacSec;

#[cfg(test)]
extern crate std;

#[cfg(test)]
std::thread_local! {
    pub static MOCK_SEC_PTR: core::cell::Cell<*const bl808_pac::sec_eng::RegisterBlock> = const { core::cell::Cell::new(core::ptr::null()) };
}

#[cfg(feature = "m0")]
pub struct Sec {
    _inner: PacSec,
}

#[cfg(feature = "m0")]
impl Sec {
    pub fn new(sec: PacSec) -> Self {
        Self { _inner: sec }
    }

    fn regs(&self) -> &bl808_pac::sec_eng::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*PacSec::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_SEC_PTR.with(|p| p.get()) };
    }

    /// Load a 128-bit (4 x u32) AES key into SE_AES_0_KEY_0..3.
    ///
    /// `key[0]` goes to SE_AES_0_KEY_0, `key[3]` to SE_AES_0_KEY_3.
    ///
    /// # Signature change vs old stub
    /// The old stub wrote to raw offset 0x80 (u32 index 32 from block base).
    /// This implementation keeps the same `&[u32; 4]` signature but routes
    /// writes through the real PAC register array `se_aes_0_key_(n)`.
    pub fn load_aes_key(&mut self, key: &[u32; 4]) {
        let regs = self.regs();
        // SAFETY: single-hart bare-metal register writes through PAC accessors.
        unsafe {
            regs.se_aes_0_key_(0).write(|w| w.se_aes_0_key().bits(key[0]));
            regs.se_aes_0_key_(1).write(|w| w.se_aes_0_key().bits(key[1]));
            regs.se_aes_0_key_(2).write(|w| w.se_aes_0_key().bits(key[2]));
            regs.se_aes_0_key_(3).write(|w| w.se_aes_0_key().bits(key[3]));
        }
    }

    /// Configure the SHA engine for SHA-256 and pulse the trigger.
    ///
    /// Sets se_sha_0_en=1, se_sha_0_mode=0 (SHA-256), then writes
    /// se_sha_0_trig_1t=1 (one-shot pulse) in SE_SHA_0_CTRL.
    ///
    /// The caller is responsible for configuring SE_SHA_0_MSA (source address)
    /// and SE_SHA_0_CTRL.se_sha_0_msg_len before calling this method.
    /// Poll [`is_busy`] to wait for completion.
    pub fn trigger_sha256(&mut self) {
        let regs = self.regs();
        // SAFETY: single-hart bare-metal register writes through PAC accessors.
        unsafe {
            // Step 1: enable engine and set mode to SHA-256 (mode field = 0).
            regs.se_sha_0_ctrl().modify(|_, w| {
                w.se_sha_0_en().set_bit();
                w.se_sha_0_mode().bits(0) // 0 = SHA-256
            });
            // Step 2: pulse the trigger (write-only field, separate modify).
            regs.se_sha_0_ctrl().modify(|_, w| {
                w.se_sha_0_trig_1t().set_bit()
            });
        }
    }

    /// Returns `true` if the SHA engine is currently busy.
    ///
    /// Maps to `se_sha_0_busy` (bit 0) in SE_SHA_0_CTRL.
    ///
    /// # Note
    /// The AES engine busy state is separate (`se_aes_0_busy` in SE_AES_0_CTRL).
    // Note: `is_aes_busy()` for `se_aes_0_busy` in SE_AES_0_CTRL is not yet
    // exposed; add it if callers need to poll AES-engine completion separately.
    pub fn is_busy(&self) -> bool {
        self.regs()
            .se_sha_0_ctrl()
            .read()
            .se_sha_0_busy()
            .bit_is_set()
    }
}

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::sec_eng::RegisterBlock;
    use std::boxed::Box;

    fn setup() -> &'static RegisterBlock {
        let mock: &'static mut [u32; 1024] = Box::leak(Box::new([0u32; 1024]));
        let ptr = mock.as_ptr() as *const RegisterBlock;
        MOCK_SEC_PTR.with(|p| p.set(ptr));
        unsafe { &*ptr }
    }

    fn make_driver() -> Sec {
        let pac = unsafe { bl808_pac::SecEng::steal() };
        Sec::new(pac)
    }

    /// load_aes_key must write each word to SE_AES_0_KEY_0..3 via the PAC
    /// accessor. We read back through the same PAC accessor to confirm the
    /// correct register offsets were hit.
    #[test]
    fn test_sec_aes_key_loading() {
        let regs = setup();
        let mut driver = make_driver();

        let key = [0x11223344u32, 0x55667788, 0x99AABBCC, 0xDDEEFF00];
        driver.load_aes_key(&key);

        assert_eq!(
            regs.se_aes_0_key_(0).read().se_aes_0_key().bits(),
            0x11223344,
            "SE_AES_0_KEY_0 should hold key word 0"
        );
        assert_eq!(
            regs.se_aes_0_key_(1).read().se_aes_0_key().bits(),
            0x55667788,
            "SE_AES_0_KEY_1 should hold key word 1"
        );
        assert_eq!(
            regs.se_aes_0_key_(2).read().se_aes_0_key().bits(),
            0x99AABBCC,
            "SE_AES_0_KEY_2 should hold key word 2"
        );
        assert_eq!(
            regs.se_aes_0_key_(3).read().se_aes_0_key().bits(),
            0xDDEEFF00,
            "SE_AES_0_KEY_3 should hold key word 3"
        );
    }

    /// trigger_sha256 must set se_sha_0_en (bit 5) and se_sha_0_mode=0
    /// (SHA-256, bits 2:4) in SE_SHA_0_CTRL. The trig_1t pulse (bit 1)
    /// is write-only so we verify the persistent enable and mode fields.
    #[test]
    fn test_sec_sha256_trigger() {
        let regs = setup();
        let mut driver = make_driver();

        driver.trigger_sha256();

        let v = regs.se_sha_0_ctrl().read();
        assert!(v.se_sha_0_en().bit_is_set(), "se_sha_0_en (bit 5) must be set");
        assert_eq!(v.se_sha_0_mode().bits(), 0, "se_sha_0_mode must be 0 (SHA-256)");
    }

    /// is_busy reads se_sha_0_busy (bit 0) from SE_SHA_0_CTRL.
    #[test]
    fn test_sec_engine_busy_poll() {
        let regs = setup();
        let driver = make_driver();

        // Set se_sha_0_busy (bit 0) via raw write into the mock.
        unsafe {
            core::ptr::write_volatile(regs.se_sha_0_ctrl().as_ptr(), 1u32);
        }
        assert!(driver.is_busy(), "should report busy when se_sha_0_busy (bit 0) is set");

        unsafe {
            core::ptr::write_volatile(regs.se_sha_0_ctrl().as_ptr(), 0u32);
        }
        assert!(!driver.is_busy(), "should not report busy when se_sha_0_busy is clear");
    }
}
