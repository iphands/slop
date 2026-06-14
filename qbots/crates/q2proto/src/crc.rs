//! CCITT CRC-16 + the Q2 usercmd checksum.
//!
//! Ports `CRC_Block` and `COM_BlockSequenceCRCByte` from `common/crc.c`. The latter is
//! the per-`clc_move` checksum the server validates — get it wrong and the server drops
//! the move (or the client).

use crate::crc_tables::{CHK_TABLE, CRC_TABLE};

const CRC_INIT: u16 = 0xffff;

/// `CRC_Block` (`crc.c:142`): CCITT CRC-16 (poly 0x1021, init 0xffff) over `data`.
pub fn crc_block(data: &[u8]) -> u16 {
    let mut crc = CRC_INIT;
    for &b in data {
        let idx = (((crc >> 8) as u8) ^ b) as usize;
        crc = (crc << 8) ^ CRC_TABLE[idx];
    }
    crc
}

/// `COM_BlockSequenceCRCByte` (`crc.c:157`): checksum over up to 60 bytes of `base`,
/// plus 4 bytes pulled from [`CHK_TABLE`] at `sequence % 1020`, CRC'd and XOR'd with the
/// byte sum. `base` is the clc_move body *after* the checksum byte; `sequence` is the
/// netchan outgoing_sequence this packet will carry.
pub fn block_sequence_crc_byte(base: &[u8], sequence: u32) -> u8 {
    let offset = (sequence as usize) % (CHK_TABLE.len() - 4); // % 1020
    let length = base.len().min(60);
    let mut chkb = [0u8; 60 + 4];
    chkb[..length].copy_from_slice(&base[..length]);
    chkb[length] = CHK_TABLE[offset];
    chkb[length + 1] = CHK_TABLE[offset + 1];
    chkb[length + 2] = CHK_TABLE[offset + 2];
    chkb[length + 3] = CHK_TABLE[offset + 3];
    let total = length + 4;

    let crc = crc_block(&chkb[..total]);
    let sum: u32 = chkb[..total].iter().map(|&b| b as u32).sum();
    ((crc as u32) ^ sum) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_block_check_value() {
        // CRC-16/CCITT-FALSE (init 0xffff, poly 0x1021) of "123456789" is 0x29B1.
        assert_eq!(crc_block(b"123456789"), 0x29b1);
    }
}
