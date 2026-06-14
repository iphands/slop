//! Append-only little-endian message builder.
//!
//! Ports the write side of yquake2's `MSG_Write*` functions (`src/common/movemsg.c`).
//! Encoding details (confirmed against that source):
//! - `i16`/`i32`/`f32` are written little-endian.
//! - **coord** = `(v * 8)` truncated to a short — fixed-point `1/8` unit.
//! - **angle** = `(v * 256 / 360)` as a byte.
//! - **dir**  = index of the [`BYTEDIRS`] entry with the greatest dot product.

use bytes::{BufMut, Bytes, BytesMut};

use crate::bytedirs::BYTEDIRS;

/// An append-only message builder over a [`BytesMut`] buffer.
pub struct Writer {
    buf: BytesMut,
}

impl Writer {
    /// Empty buffer.
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    /// Empty buffer with `n` bytes pre-allocated.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            buf: BytesMut::with_capacity(n),
        }
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The written bytes, borrowed.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Consume the writer, returning the immutable [`Bytes`].
    pub fn freeze(self) -> Bytes {
        self.buf.freeze()
    }

    /// `MSG_WriteByte`.
    pub fn write_u8(&mut self, v: u8) {
        self.buf.put_u8(v);
    }

    /// `MSG_WriteChar`.
    pub fn write_i8(&mut self, v: i8) {
        self.buf.put_i8(v);
    }

    /// `MSG_WriteShort` — writes the low 16 bits, little-endian.
    pub fn write_i16(&mut self, v: i16) {
        self.buf.put_i16_le(v);
    }

    /// `MSG_WriteLong` — little-endian.
    pub fn write_i32(&mut self, v: i32) {
        self.buf.put_i32_le(v);
    }

    /// `MSG_WriteFloat` — IEEE-754 float, little-endian.
    pub fn write_f32(&mut self, v: f32) {
        self.buf.put_f32_le(v);
    }

    /// `SZ_Write` — raw bytes.
    pub fn write_bytes(&mut self, b: &[u8]) {
        self.buf.put_slice(b);
    }

    /// `MSG_WriteString` — bytes followed by a NUL terminator.
    pub fn write_string(&mut self, s: &str) {
        self.buf.put_slice(s.as_bytes());
        self.buf.put_u8(0);
    }

    /// `MSG_WriteCoord` — `(v * 8)` as a short (Q2 fixed-point).
    pub fn write_coord(&mut self, v: f32) {
        // float→i32 truncates toward zero (matches C `(int)`); i32→i16 takes the low
        // 16 bits (matches the C short store). Real map coords never overflow i16/8.
        self.write_i16((v * 8.0) as i32 as i16);
    }

    /// `MSG_WritePos` — three coords.
    pub fn write_pos(&mut self, p: [f32; 3]) {
        self.write_coord(p[0]);
        self.write_coord(p[1]);
        self.write_coord(p[2]);
    }

    /// `MSG_WriteAngle` — `(v * 256 / 360)` as a byte.
    pub fn write_angle(&mut self, v: f32) {
        self.write_u8(((v * 256.0 / 360.0) as i32) as u8);
    }

    /// `MSG_WriteAngle16` — `ANGLE2SHORT(v)` as a short.
    pub fn write_angle16(&mut self, v: f32) {
        self.write_i16(((v * 65536.0 / 360.0) as i32) as i16);
    }

    /// `MSG_WriteDir` — index of the [`BYTEDIRS`] entry nearest to `v` (max dot
    /// product). `v` should be normalized by the caller. Falls back to index 0 when no
    /// direction has positive dot (matches C's `bestd = 0` initialization).
    pub fn write_dir(&mut self, v: [f32; 3]) {
        let mut best = 0u8;
        let mut bestd = 0.0f32;
        for (i, d) in BYTEDIRS.iter().enumerate() {
            let dot = v[0] * d[0] + v[1] * d[1] + v[2] * d[2];
            if dot > bestd {
                bestd = dot;
                best = i as u8;
            }
        }
        self.write_u8(best);
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_primitives_bytes() {
        let mut w = Writer::new();
        w.write_u8(0x01);
        w.write_i8(-1);
        w.write_i16(0x0102);
        w.write_i32(0x12345678);
        assert_eq!(
            w.as_bytes(),
            &[0x01, 0xff, 0x02, 0x01, 0x78, 0x56, 0x34, 0x12]
        );
    }

    #[test]
    fn write_string_nul_terminated() {
        let mut w = Writer::new();
        w.write_string("hi");
        assert_eq!(w.as_bytes(), b"hi\0");
    }

    #[test]
    fn coord_bytes_match_c() {
        let mut w = Writer::new();
        w.write_coord(3.0); // 3*8 = 24 = 0x0018 LE → [0x18, 0x00]
        assert_eq!(w.as_bytes(), &[0x18, 0x00]);
    }

    #[test]
    fn angle_bytes_match_c() {
        let mut w = Writer::new();
        w.write_angle(180.0); // 180*256/360 = 128
        assert_eq!(w.as_bytes(), &[128]);
    }
}
