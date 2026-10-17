//! NEC infrared protocol — pure Rust encode/decode (no PAC dependency).
//!
//! These functions operate on the 32-bit on-wire NEC frame and are fully
//! portable; they run in host unit tests without any hardware feature flags.
//!
//! # Frame layout
//!
//! ```text
//! bits  0– 7 : address
//! bits  8–15 : ~address (complement)
//! bits 16–23 : command
//! bits 24–31 : ~command (complement)
//! ```
//!
//! All bits are transmitted LSB-first over the wire.

/// Encode an NEC `(addr, cmd)` pair into the 32-bit on-wire frame.
#[inline]
pub fn nec_frame(addr: u8, cmd: u8) -> u32 {
    (addr as u32)
        | ((!addr as u32) << 8)
        | ((cmd as u32) << 16)
        | ((!cmd as u32) << 24)
}

/// Decode a 32-bit NEC frame received from hardware.
///
/// Returns `Some((addr, cmd))` when both byte/complement pairs pass their
/// XOR check, `None` on any checksum failure.
#[inline]
pub fn decode_nec(frame: u32) -> Option<(u8, u8)> {
    let addr = (frame & 0xFF) as u8;
    let addr_inv = ((frame >> 8) & 0xFF) as u8;
    let cmd = ((frame >> 16) & 0xFF) as u8;
    let cmd_inv = ((frame >> 24) & 0xFF) as u8;

    if addr ^ addr_inv == 0xFF && cmd ^ cmd_inv == 0xFF {
        Some((addr, cmd))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate std;

    #[test]
    fn test_nec_frame_known_value() {
        // addr=0x00, cmd=0x01 → 0x00 | 0xFF<<8 | 0x01<<16 | 0xFE<<24
        assert_eq!(nec_frame(0x00, 0x01), 0xFE01_FF00);
    }

    #[test]
    fn test_nec_frame_byte_layout() {
        let addr: u8 = 0x59;
        let cmd: u8 = 0xA3;
        let frame = nec_frame(addr, cmd);
        assert_eq!((frame & 0xFF) as u8, addr);
        assert_eq!(((frame >> 8) & 0xFF) as u8, !addr);
        assert_eq!(((frame >> 16) & 0xFF) as u8, cmd);
        assert_eq!(((frame >> 24) & 0xFF) as u8, !cmd);
    }

    #[test]
    fn test_decode_nec_valid() {
        assert_eq!(decode_nec(nec_frame(0x59, 0xA3)), Some((0x59, 0xA3)));
    }

    #[test]
    fn test_decode_nec_zero() {
        assert_eq!(decode_nec(nec_frame(0, 0)), Some((0, 0)));
    }

    #[test]
    fn test_decode_nec_all_ones() {
        assert_eq!(decode_nec(nec_frame(0xFF, 0xFF)), Some((0xFF, 0xFF)));
    }

    #[test]
    fn test_decode_nec_bad_addr_complement() {
        let frame = nec_frame(0x59, 0xA3) ^ 0x0000_0100;
        assert_eq!(decode_nec(frame), None);
    }

    #[test]
    fn test_decode_nec_bad_cmd_complement() {
        let frame = nec_frame(0x59, 0xA3) ^ 0x0100_0000;
        assert_eq!(decode_nec(frame), None);
    }

    #[test]
    fn test_decode_nec_roundtrip_sweep() {
        for addr in [0u8, 1, 0x7F, 0x80, 0xFE, 0xFF] {
            for cmd in [0u8, 1, 0x7F, 0x80, 0xFE, 0xFF] {
                let frame = nec_frame(addr, cmd);
                assert_eq!(
                    decode_nec(frame),
                    Some((addr, cmd)),
                    "roundtrip failed for addr={addr:#04x} cmd={cmd:#04x}"
                );
            }
        }
    }
}
