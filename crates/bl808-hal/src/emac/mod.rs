//! Ethernet MAC (EMAC) Driver
//!
//! Implements TX/RX buffer descriptor (BD) ring management for the BL808
//! Ethernet MAC (OpenCores ETHOC-compatible).
//!
//! # Buffer Descriptor Layout (16 bytes / 4 × u32 each)
//!
//! | Word | Field    | TX meaning                   | RX meaning                    |
//! |------|----------|------------------------------|-------------------------------|
//! | 0    | status   | Control flags + HW-own bit   | Status flags + HW-empty bit   |
//! | 1    | length   | Frame length in bytes        | Received frame length         |
//! | 2    | buf_ptr  | Physical addr of data buffer | Physical addr of data buffer  |
//! | 3    | next_ptr | Reserved                     | Reserved                      |
//!
//! TX BDs occupy indices `0 .. TX_BD_COUNT`.  
//! RX BDs occupy indices `TX_BD_COUNT .. TX_BD_COUNT + RX_BD_COUNT`.

use bl808_pac::Emac;

#[cfg(test)]
extern crate std;

// ── BD configuration ─────────────────────────────────────────────────────────

/// Number of TX buffer descriptors.
pub const TX_BD_COUNT: usize = 8;
/// Number of RX buffer descriptors.
pub const RX_BD_COUNT: usize = 8;
/// u32 words per BD (4 × u32 = 16 bytes).
const WORDS_PER_BD: usize = 4;

// BD word indices.
const BD_STATUS: usize = 0;
const BD_LEN: usize = 1;
const BD_BUF: usize = 2;
// BD word 3 (next_ptr) is reserved and not accessed by the driver.

// TX BD status bits.
/// Hardware owns the BD (transmission in progress).
const TX_BD_READY: u32 = 1 << 15;
/// Generate interrupt on completion.
const TX_BD_IRQ: u32 = 1 << 14;
/// Wrap: last BD in ring — return to BD 0 after this one.
const TX_BD_WRAP: u32 = 1 << 13;
/// Pad frame to minimum Ethernet length (64 bytes).
const TX_BD_PAD: u32 = 1 << 12;
/// Append FCS/CRC.
const TX_BD_CRC: u32 = 1 << 11;

// RX BD status bits.
/// Hardware owns the BD (waiting to receive a frame).
const RX_BD_EMPTY: u32 = 1 << 15;
/// Generate interrupt on reception.
const RX_BD_IRQ: u32 = 1 << 14;
/// Wrap: last BD in ring.
const RX_BD_WRAP: u32 = 1 << 13;
/// Error mask: OR, IS, DN, TL, SF, CRC, LC bits.
const RX_BD_ERR: u32 = (1 << 6) | (1 << 4) | (1 << 3) | (1 << 2) | (1 << 1) | 1;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors returned by EMAC driver operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmacError {
    /// MDIO operation timed out.
    MdioTimeout,
    /// DMA / BD-ring error.
    DmaError,
    /// All TX BDs are owned by hardware; ring is full.
    TxRingFull,
    /// Spin-poll timed out waiting for hardware.
    Timeout,
    /// Received frame had one or more error flags set.
    RxError,
}

// ── Mock (test only) ──────────────────────────────────────────────────────────

#[cfg(test)]
std::thread_local! {
    /// Points to a heap-allocated mock of the EMAC PAC register block.
    pub static MOCK_EMAC_PTR: core::cell::Cell<*const bl808_pac::emac::RegisterBlock> =
        const { core::cell::Cell::new(core::ptr::null()) };
    /// Points to a heap-allocated mock of the BD memory region.
    pub static MOCK_EMAC_BD_PTR: core::cell::Cell<*mut u32> =
        const { core::cell::Cell::new(core::ptr::null_mut()) };
}

// ── Driver ────────────────────────────────────────────────────────────────────

/// Ethernet MAC driver.
pub struct EthernetMac {
    _emac: Emac,
    /// Base of BD memory: TX_BD_COUNT BDs followed by RX_BD_COUNT BDs.
    bd_base: *mut u32,
    /// Index of the next TX BD to fill (0 .. TX_BD_COUNT).
    tx_head: usize,
    /// Index of the next RX BD to check for a received frame (0 .. RX_BD_COUNT).
    rx_head: usize,
}

// SAFETY: On single-core M0 this is always true. The raw pointer to BD memory
// points to statically-allocated DMA-coherent memory that outlives the driver.
unsafe impl Send for EthernetMac {}

impl EthernetMac {
    /// Create an Ethernet MAC driver and initialise the RX BD ring.
    ///
    /// # Arguments
    /// * `emac`    — PAC peripheral singleton.
    /// * `bd_base` — Pointer to DMA-coherent memory for buffer descriptors.
    ///   Must provide at least
    ///   `(TX_BD_COUNT + RX_BD_COUNT) * 4 * size_of::<u32>()` bytes, 16-byte aligned.
    ///
    /// # Safety
    /// `bd_base` must be valid and stable in DMA-accessible memory for the
    /// entire lifetime of this struct.
    pub unsafe fn new(emac: Emac, bd_base: *mut u32) -> Self {
        let mut driver = Self {
            _emac: emac,
            bd_base,
            tx_head: 0,
            rx_head: 0,
        };
        driver.init_rx_ring();
        driver
    }

    #[inline]
    fn regs(&self) -> &bl808_pac::emac::RegisterBlock {
        #[cfg(not(test))]
        return unsafe { &*Emac::PTR };
        #[cfg(test)]
        return unsafe { &*MOCK_EMAC_PTR.with(|p| p.get()) };
    }

    /// Return a pointer to the start of BD `absolute_idx`.
    #[inline]
    fn bd_ptr(&self, absolute_idx: usize) -> *mut u32 {
        unsafe { self.bd_base.add(absolute_idx * WORDS_PER_BD) }
    }

    #[inline]
    fn tx_bd(&self, idx: usize) -> *mut u32 {
        self.bd_ptr(idx)
    }

    #[inline]
    fn rx_bd(&self, idx: usize) -> *mut u32 {
        self.bd_ptr(TX_BD_COUNT + idx)
    }

    /// Mark all RX BDs as hardware-owned (EMPTY) with IRQ enabled.
    /// Callers must populate BD_BUF via [`set_rx_buf`] before enabling the MAC.
    fn init_rx_ring(&mut self) {
        for i in 0..RX_BD_COUNT {
            let wrap = if i == RX_BD_COUNT - 1 { RX_BD_WRAP } else { 0 };
            let bd = self.rx_bd(i);
            unsafe {
                bd.add(BD_STATUS).write_volatile(RX_BD_EMPTY | RX_BD_IRQ | wrap);
                bd.add(BD_LEN).write_volatile(0);
                // BD_BUF intentionally left at 0; caller sets it per BD.
            }
        }
    }

    // ── Initialisation ────────────────────────────────────────────────────────

    /// Configure the number of TX BDs in the hardware register.
    ///
    /// Must be called once before [`enable`].
    pub fn configure(&mut self) {
        self.regs()
            .tx_bd_num()
            .modify(|_, w| unsafe { w.txbdnum().bits(TX_BD_COUNT as u8) });
    }

    /// Set the station MAC address.
    ///
    /// `addr` is in network byte order (addr[0] = first transmitted octet).
    ///
    /// Hardware layout (confirmed in bl_mcu_sdk_bl808/drivers/bl808_driver/std_drv/src/bl808_emac.c,
    /// `EMAC_SetMACAddress`, lines 101-102):
    ///   MAC_ADDR1 (offset 0x44) = (addr[0] << 8) | addr[1]
    ///   MAC_ADDR0 (offset 0x40) = (addr[2] << 24) | (addr[3] << 16) | (addr[4] << 8) | addr[5]
    pub fn set_mac_addr(&mut self, addr: [u8; 6]) {
        let regs = self.regs();
        // MAC_ADDR0 holds bytes [2,3,4,5] — addr[2] in the MSB.
        let a0 = ((addr[2] as u32) << 24)
            | ((addr[3] as u32) << 16)
            | ((addr[4] as u32) << 8)
            | (addr[5] as u32);
        // MAC_ADDR1 holds bytes [0,1] — addr[0] in bits [15:8].
        let a1 = ((addr[0] as u32) << 8) | (addr[1] as u32);
        regs.mac_addr0().write(|w| unsafe { w.bits(a0) });
        regs.mac_addr1().write(|w| unsafe { w.bits(a1) });
    }

    /// Enable the MAC receiver and transmitter.
    pub fn enable(&mut self) {
        self.regs()
            .mode()
            .modify(|_, w| w.rxen().set_bit().txen().set_bit());
    }

    /// Disable the MAC receiver and transmitter.
    pub fn disable(&mut self) {
        self.regs()
            .mode()
            .modify(|_, w| w.rxen().clear_bit().txen().clear_bit());
    }

    // ── MDIO ─────────────────────────────────────────────────────────────────

    /// Perform a synchronous MDIO register write.
    pub fn mdio_write(&mut self, phy: u8, reg: u8, data: u16) -> Result<(), EmacError> {
        let regs = self.regs();
        regs.miitx_data()
            .write(|w| unsafe { w.ctrldata().bits(data) });
        regs.miiaddress()
            .write(|w| unsafe { w.fiad().bits(phy).rgad().bits(reg) });
        regs.miicommand().write(|w| w.wctrldata().set_bit());
        Ok(())
    }

    // ── RX ring ───────────────────────────────────────────────────────────────

    /// Set the DMA buffer address for RX BD at `idx`.
    ///
    /// Call once per RX BD before calling [`enable`], providing a buffer large
    /// enough for the maximum expected Ethernet frame (≥1518 bytes).
    ///
    /// # Safety
    /// `buf_phys` must be the physical address of DMA-accessible memory that
    /// remains valid while the MAC is running.
    pub unsafe fn set_rx_buf(&mut self, idx: usize, buf_phys: u32) {
        debug_assert!(idx < RX_BD_COUNT);
        unsafe { self.rx_bd(idx).add(BD_BUF).write_volatile(buf_phys) };
    }

    /// Poll the RX ring for a received frame.
    ///
    /// Returns `Ok(Some((buf_phys, len)))` when a frame is available, where
    /// `buf_phys` is the physical address of the data and `len` is its byte
    /// count. The BD is immediately returned to hardware after reading.
    ///
    /// Returns `Ok(None)` when the ring is empty, or `Err(EmacError::RxError)`
    /// on a frame with hardware-reported errors (BD is still returned).
    pub fn poll_rx(&mut self) -> Result<Option<(u32, usize)>, EmacError> {
        let bd = self.rx_bd(self.rx_head);

        // SAFETY: bd_base is valid DMA-coherent memory.
        let status = unsafe { bd.add(BD_STATUS).read_volatile() };
        if status & RX_BD_EMPTY != 0 {
            return Ok(None); // hardware still owns this BD
        }

        // Read frame metadata before returning BD to hardware.
        let len = (unsafe { bd.add(BD_LEN).read_volatile() } & 0xFFFF) as usize;
        let buf_phys = unsafe { bd.add(BD_BUF).read_volatile() };
        let had_error = status & RX_BD_ERR != 0;

        // Return BD to hardware, preserving the buffer pointer.
        let wrap = if self.rx_head == RX_BD_COUNT - 1 {
            RX_BD_WRAP
        } else {
            0
        };
        unsafe {
            bd.add(BD_STATUS).write_volatile(RX_BD_EMPTY | RX_BD_IRQ | wrap);
        }
        self.rx_head = (self.rx_head + 1) % RX_BD_COUNT;

        if had_error {
            Err(EmacError::RxError)
        } else {
            Ok(Some((buf_phys, len)))
        }
    }

    // ── TX ring ───────────────────────────────────────────────────────────────

    /// Enqueue a frame for transmission.
    ///
    /// Hands the next free TX BD to hardware with `buf`'s address and length.
    /// Returns `Err(EmacError::TxRingFull)` if all TX BDs are owned by hardware.
    ///
    /// # Safety
    /// `buf` must remain valid and stable in memory until transmission
    /// completes, verified by calling [`flush_tx`].
    pub unsafe fn enqueue_tx(&mut self, buf: &[u8]) -> Result<(), EmacError> {
        let bd = self.tx_bd(self.tx_head);
        let status = unsafe { bd.add(BD_STATUS).read_volatile() };
        if status & TX_BD_READY != 0 {
            return Err(EmacError::TxRingFull);
        }

        let wrap = if self.tx_head == TX_BD_COUNT - 1 {
            TX_BD_WRAP
        } else {
            0
        };

        unsafe {
            bd.add(BD_BUF).write_volatile(buf.as_ptr() as u32);
            bd.add(BD_LEN).write_volatile(buf.len() as u32);
            // Write READY last: this is the hand-off to hardware.
            bd.add(BD_STATUS)
                .write_volatile(TX_BD_READY | TX_BD_IRQ | TX_BD_CRC | TX_BD_PAD | wrap);
        }

        self.tx_head = (self.tx_head + 1) % TX_BD_COUNT;
        Ok(())
    }

    /// Block until all enqueued TX BDs have been released by hardware.
    ///
    /// Returns `Err(EmacError::Timeout)` if any BD remains busy after the
    /// spin-poll limit.
    pub fn flush_tx(&self) -> Result<(), EmacError> {
        for i in 0..TX_BD_COUNT {
            let bd = self.tx_bd(i);
            let mut budget = 10_000_000u32;
            loop {
                let status = unsafe { bd.add(BD_STATUS).read_volatile() };
                if status & TX_BD_READY == 0 {
                    break;
                }
                budget -= 1;
                if budget == 0 {
                    return Err(EmacError::Timeout);
                }
                core::hint::spin_loop();
            }
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "m0"))]
mod tests {
    use super::*;
    use bl808_pac::emac::RegisterBlock;
    use std::boxed::Box;

    /// Allocate mock register and BD memory, return a fresh `EthernetMac`.
    fn make_mac() -> EthernetMac {
        // PAC register mock: RegisterBlock is 0x54 bytes; 32 u32s is enough.
        let reg_mock: &'static mut [u32; 32] = Box::leak(Box::new([0u32; 32]));
        MOCK_EMAC_PTR.with(|p| p.set(reg_mock.as_ptr() as *const RegisterBlock));

        // BD memory: (TX_BD_COUNT + RX_BD_COUNT) BDs × WORDS_PER_BD u32s.
        let bd_words = (TX_BD_COUNT + RX_BD_COUNT) * WORDS_PER_BD;
        let bd_mock: &'static mut std::vec::Vec<u32> =
            Box::leak(Box::new(std::vec![0u32; bd_words]));
        MOCK_EMAC_BD_PTR.with(|p| p.set(bd_mock.as_mut_ptr()));

        let emac = unsafe { Emac::steal() };
        unsafe { EthernetMac::new(emac, bd_mock.as_mut_ptr()) }
    }

    fn read_bd(bd_idx: usize, word: usize) -> u32 {
        let base = MOCK_EMAC_BD_PTR.with(|p| p.get());
        unsafe { base.add(bd_idx * WORDS_PER_BD + word).read_volatile() }
    }

    fn write_bd(bd_idx: usize, word: usize, val: u32) {
        let base = MOCK_EMAC_BD_PTR.with(|p| p.get());
        unsafe { base.add(bd_idx * WORDS_PER_BD + word).write_volatile(val) }
    }

    // ── RX ring initialisation ─────────────────────────────────────────────

    #[test]
    fn test_rx_ring_init_all_empty() {
        let _mac = make_mac();
        for i in 0..RX_BD_COUNT {
            let status = read_bd(TX_BD_COUNT + i, BD_STATUS);
            assert_ne!(status & RX_BD_EMPTY, 0, "RX BD {i}: EMPTY should be set");
            assert_ne!(status & RX_BD_IRQ, 0, "RX BD {i}: IRQ should be set");
        }
    }

    #[test]
    fn test_rx_ring_last_bd_has_wrap() {
        let _mac = make_mac();
        let last = TX_BD_COUNT + RX_BD_COUNT - 1;
        let status = read_bd(last, BD_STATUS);
        assert_ne!(status & RX_BD_WRAP, 0, "Last RX BD should have WRAP set");
    }

    #[test]
    fn test_rx_ring_non_last_bds_no_wrap() {
        let _mac = make_mac();
        for i in 0..RX_BD_COUNT - 1 {
            let status = read_bd(TX_BD_COUNT + i, BD_STATUS);
            assert_eq!(status & RX_BD_WRAP, 0, "RX BD {i} should not have WRAP");
        }
    }

    // ── TX ring ────────────────────────────────────────────────────────────

    #[test]
    fn test_enqueue_tx_sets_ready_crc_pad_irq() {
        let mut mac = make_mac();
        let data = [0xAAu8; 64];
        unsafe { mac.enqueue_tx(&data) }.unwrap();

        let status = read_bd(0, BD_STATUS);
        assert_ne!(status & TX_BD_READY, 0, "READY should be set");
        assert_ne!(status & TX_BD_CRC, 0, "CRC should be set");
        assert_ne!(status & TX_BD_PAD, 0, "PAD should be set");
        assert_ne!(status & TX_BD_IRQ, 0, "IRQ should be set");
    }

    #[test]
    fn test_enqueue_tx_stores_buf_ptr_and_len() {
        let mut mac = make_mac();
        let data = [0xBBu8; 100];
        unsafe { mac.enqueue_tx(&data) }.unwrap();

        assert_eq!(read_bd(0, BD_BUF), data.as_ptr() as u32);
        assert_eq!(read_bd(0, BD_LEN), 100);
    }

    #[test]
    fn test_enqueue_tx_advances_head() {
        let mut mac = make_mac();
        let d1 = [0x11u8; 64];
        let d2 = [0x22u8; 64];
        unsafe { mac.enqueue_tx(&d1) }.unwrap();
        unsafe { mac.enqueue_tx(&d2) }.unwrap();

        assert_eq!(read_bd(1, BD_BUF), d2.as_ptr() as u32, "Second frame in BD 1");
    }

    #[test]
    fn test_enqueue_tx_last_bd_has_wrap() {
        let mut mac = make_mac();
        let data = [0u8; 64];
        for _ in 0..TX_BD_COUNT {
            unsafe { mac.enqueue_tx(&data) }.unwrap();
        }
        let last_status = read_bd(TX_BD_COUNT - 1, BD_STATUS);
        assert_ne!(last_status & TX_BD_WRAP, 0, "Last TX BD should have WRAP");
    }

    #[test]
    fn test_enqueue_tx_full_ring_returns_error() {
        let mut mac = make_mac();
        let data = [0u8; 64];
        for _ in 0..TX_BD_COUNT {
            unsafe { mac.enqueue_tx(&data) }.unwrap();
        }
        assert_eq!(
            unsafe { mac.enqueue_tx(&data) },
            Err(EmacError::TxRingFull)
        );
    }

    // ── RX polling ─────────────────────────────────────────────────────────

    #[test]
    fn test_poll_rx_none_when_ring_empty() {
        let mut mac = make_mac();
        assert_eq!(mac.poll_rx(), Ok(None));
    }

    #[test]
    fn test_poll_rx_returns_frame_when_ready() {
        let mut mac = make_mac();
        let rx_bd0 = TX_BD_COUNT;

        // Simulate hardware delivering a 100-byte frame.
        let fake_phys: u32 = 0x8000_0000;
        write_bd(rx_bd0, BD_BUF, fake_phys);
        write_bd(rx_bd0, BD_LEN, 100);
        write_bd(rx_bd0, BD_STATUS, RX_BD_IRQ); // EMPTY clear → SW owns it

        assert_eq!(mac.poll_rx(), Ok(Some((fake_phys, 100))));
    }

    #[test]
    fn test_poll_rx_returns_bd_to_hw_after_read() {
        let mut mac = make_mac();
        let rx_bd0 = TX_BD_COUNT;

        write_bd(rx_bd0, BD_BUF, 0x8000_0000);
        write_bd(rx_bd0, BD_LEN, 64);
        write_bd(rx_bd0, BD_STATUS, 0); // EMPTY clear

        mac.poll_rx().unwrap();

        let status = read_bd(rx_bd0, BD_STATUS);
        assert_ne!(status & RX_BD_EMPTY, 0, "BD should be returned to HW");
    }

    #[test]
    fn test_poll_rx_error_frame_returns_err_and_recycles_bd() {
        let mut mac = make_mac();
        let rx_bd0 = TX_BD_COUNT;

        // Set a CRC error flag (bit 0), EMPTY clear.
        write_bd(rx_bd0, BD_STATUS, 0x0001);
        write_bd(rx_bd0, BD_LEN, 64);

        assert_eq!(mac.poll_rx(), Err(EmacError::RxError));

        let status = read_bd(rx_bd0, BD_STATUS);
        assert_ne!(status & RX_BD_EMPTY, 0, "Error BD should be recycled to HW");
    }

    #[test]
    fn test_poll_rx_advances_rx_head() {
        let mut mac = make_mac();
        let rx_bd0 = TX_BD_COUNT;
        let rx_bd1 = TX_BD_COUNT + 1;

        // First frame in BD 0.
        write_bd(rx_bd0, BD_BUF, 0x8000_0000);
        write_bd(rx_bd0, BD_LEN, 60);
        write_bd(rx_bd0, BD_STATUS, 0);

        // Second frame in BD 1.
        write_bd(rx_bd1, BD_BUF, 0x8000_1000);
        write_bd(rx_bd1, BD_LEN, 80);
        write_bd(rx_bd1, BD_STATUS, 0);

        mac.poll_rx().unwrap();
        let result = mac.poll_rx().unwrap();
        assert_eq!(result, Some((0x8000_1000, 80)), "Second poll should read BD 1");
    }

    // ── MAC configuration ──────────────────────────────────────────────────

    #[test]
    fn test_configure_sets_tx_bd_num() {
        let mut mac = make_mac();
        mac.configure();

        let ptr = MOCK_EMAC_PTR.with(|p| p.get()) as *const u32;
        // tx_bd_num is at offset 0x20 in RegisterBlock.
        let tx_bd_num = unsafe { core::ptr::read_volatile(ptr.add(0x20 / 4)) };
        assert_eq!(
            tx_bd_num & 0xFF,
            TX_BD_COUNT as u32,
            "txbdnum field should equal TX_BD_COUNT"
        );
    }

    #[test]
    fn test_mdio_write_triggers_wctrldata() {
        let mut mac = make_mac();
        mac.mdio_write(1, 2, 0xABCD).unwrap();

        let ptr = MOCK_EMAC_PTR.with(|p| p.get()) as *const u32;
        // miicommand is at offset 0x2c.
        let cmd = unsafe { core::ptr::read_volatile(ptr.add(0x2C / 4)) };
        assert_ne!(cmd & 4, 0, "WCTRLDATA (bit 2) must be set");
    }

    #[test]
    fn test_set_mac_addr_writes_correct_registers() {
        let mut mac = make_mac();
        let addr = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        mac.set_mac_addr(addr);

        // SDK (bl808_emac.c EMAC_SetMACAddress):
        //   MAC_ADDR1 = (addr[0]<<8) | addr[1]  => (0x00<<8)|0x11 = 0x0011
        //   MAC_ADDR0 = (addr[2]<<24)|(addr[3]<<16)|(addr[4]<<8)|addr[5] => 0x2233_4455
        let regs = unsafe { &*MOCK_EMAC_PTR.with(|p| p.get()) };
        assert_eq!(regs.mac_addr0().read().bits(), 0x2233_4455, "MAC_ADDR0: (addr[2]<<24)|(addr[3]<<16)|(addr[4]<<8)|addr[5]");
        assert_eq!(regs.mac_addr1().read().bits(), 0x0011, "MAC_ADDR1: (addr[0]<<8)|addr[1]");
    }
}
