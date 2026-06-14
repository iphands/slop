//! Little-endian read cursor over an immutable byte buffer.
//!
//! Ports the read side of yquake2's `MSG_Read*` functions (`src/common/movemsg.c`).
//! Encoding details (confirmed against that source):
//! - `i16`/`i32`/`f32` are little-endian.
//! - **coord** = `i16 * 0.125` (fixed-point, `1/8` unit) — `MSG_ReadCoord`.
//! - **angle** = `i8 * 1.40625` (= `360/256`) — `MSG_ReadAngle` reads a *signed* byte.
//! - **angle16** = `i16 * (360/65536)` — `SHORT2ANGLE(MSG_ReadShort)`.

use crate::bytedirs::{BYTEDIRS, NUM_VERTEX_NORMALS};
use crate::error::DecodeError;

/// A read cursor over a borrowed byte slice.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Wrap a borrowed packet/message buffer at position 0.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The full underlying buffer.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Current read position (bytes consumed).
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Bytes left to read.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Advance the cursor past `n` bytes without copying them.
    pub fn skip(&mut self, n: usize) -> Result<(), DecodeError> {
        self.take(n).map(|_| ())
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + n > self.data.len() {
            self.pos = self.data.len();
            return Err(DecodeError::Eof);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// `MSG_ReadByte` — unsigned byte.
    pub fn read_u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    /// `MSG_ReadChar` — signed byte.
    pub fn read_i8(&mut self) -> Result<i8, DecodeError> {
        Ok(self.read_u8()? as i8)
    }

    /// `MSG_ReadShort` — little-endian signed 16-bit.
    pub fn read_i16(&mut self) -> Result<i16, DecodeError> {
        let b = self.take(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    /// `MSG_ReadLong` — little-endian signed 32-bit.
    pub fn read_i32(&mut self) -> Result<i32, DecodeError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// `MSG_ReadFloat` — little-endian IEEE-754 float.
    pub fn read_f32(&mut self) -> Result<f32, DecodeError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// `MSG_ReadData` — copy `n` raw bytes.
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        self.take(n)
    }

    /// `MSG_ReadString` — NUL-terminated, decoded lossily as UTF-8. Reading past the
    /// end stops the string (matches C, which halts on the `-1` sentinel).
    pub fn read_string(&mut self) -> Result<String, DecodeError> {
        let mut bytes = Vec::new();
        loop {
            match self.read_u8() {
                Ok(0) => break,
                Ok(b) => bytes.push(b),
                Err(_) => break, // EOF: keep what we have
            }
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `MSG_ReadStringLine` — stops at NUL or newline.
    pub fn read_string_line(&mut self) -> Result<String, DecodeError> {
        let mut bytes = Vec::new();
        loop {
            match self.read_u8() {
                Ok(0) | Ok(b'\n') => break,
                Ok(b) => bytes.push(b),
                Err(_) => break,
            }
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `MSG_ReadCoord` — `i16 * 0.125`.
    pub fn read_coord(&mut self) -> Result<f32, DecodeError> {
        Ok(self.read_i16()? as f32 * 0.125)
    }

    /// `MSG_ReadPos` — three coords.
    pub fn read_pos(&mut self) -> Result<[f32; 3], DecodeError> {
        Ok([self.read_coord()?, self.read_coord()?, self.read_coord()?])
    }

    /// `MSG_ReadAngle` — signed byte × `360/256`.
    pub fn read_angle(&mut self) -> Result<f32, DecodeError> {
        Ok(self.read_i8()? as f32 * 1.40625)
    }

    /// `MSG_ReadAngle16` — `SHORT2ANGLE(short)` = `short * 360/65536`.
    pub fn read_angle16(&mut self) -> Result<f32, DecodeError> {
        Ok(self.read_i16()? as f32 * (360.0 / 65536.0))
    }

    /// `MSG_ReadDir` — byte index into [`BYTEDIRS`].
    pub fn read_dir(&mut self) -> Result<[f32; 3], DecodeError> {
        let b = self.read_u8()? as usize;
        if b >= NUM_VERTEX_NORMALS {
            return Err(DecodeError::Invalid("dir index"));
        }
        Ok(BYTEDIRS[b])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(data: &[u8]) -> Reader<'_> {
        Reader::new(data)
    }

    #[test]
    fn read_primitives() {
        let data = [0x01u8, 0xff, 0x02, 0x01, 0x78, 0x56, 0x34, 0x12];
        let mut rd = r(&data);
        assert_eq!(rd.read_u8().unwrap(), 0x01);
        assert_eq!(rd.read_i8().unwrap(), -1); // 0xff as i8
        assert_eq!(rd.read_i16().unwrap(), 0x0102); // LE: 0x02,0x01
        assert_eq!(rd.read_i32().unwrap(), 0x12345678); // LE: 78 56 34 12
        assert_eq!(rd.remaining(), 0);
        assert_eq!(rd.read_u8().unwrap_err(), DecodeError::Eof);
    }

    #[test]
    fn read_float_le() {
        let mut w = crate::Writer::new();
        w.write_f32(1.5);
        let b = w.freeze();
        let mut rd = r(&b);
        assert_eq!(rd.read_f32().unwrap(), 1.5);
    }

    #[test]
    fn read_string_stops_at_nul_and_eof() {
        let mut rd = r(b"hi\0ignored");
        assert_eq!(rd.read_string().unwrap(), "hi");
        // remaining buffer now starts at "ignored"; read past end is tolerated
        let mut rd2 = r(b"abc");
        rd2.pos = 3;
        assert_eq!(rd2.read_string().unwrap(), ""); // EOF → empty
    }

    #[test]
    fn coord_and_angle_scales() {
        let mut w = crate::Writer::new();
        w.write_coord(3.0); // 3*8 = 24
        w.write_pos([1.0, 2.0, 3.0]);
        w.write_angle(90.0); // 90*256/360 = 64
        let bytes = w.freeze();
        let mut rd = r(&bytes);
        assert_eq!(rd.read_coord().unwrap(), 3.0);
        let p = rd.read_pos().unwrap();
        assert_eq!(p, [1.0, 2.0, 3.0]);
        // 90° → byte 64 → 64 * 1.40625 = 90.0.
        // Note: 180° would wrap to -180° (byte 128 read as signed i8 -128). That is
        // faithful to C's MSG_ReadAngle (which uses ReadChar) — 180 ≡ -180 mod 360.
        assert_eq!(rd.read_angle().unwrap(), 90.0);
    }

    #[test]
    fn dir_round_trips_to_nearest() {
        let up = [0.0, 0.0, 1.0];
        let mut w = crate::Writer::new();
        w.write_dir(up);
        let bytes = w.freeze();
        let mut rd = r(&bytes);
        let got = rd.read_dir().unwrap();
        // nearest bytedir to +Z must be (very nearly) +Z itself
        assert!(got[2] > 0.99 && got[2] <= 1.0);
    }
}
