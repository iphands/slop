//! Q2 `.pak` archive reader — extracts files (e.g. BSPs) from `pak0/pak1.pak`.
//!
//! Format (`files.h:30`): a header `[magic "PACK", dirofs, dirlen]` followed by a
//! directory of 64-byte `dpackfile_t` entries `[name[56], filepos, filelen]`. The stock
//! deathmatch maps (`q2dm1`…`q2dm8`) live in `pak1.pak`, not as loose files.

use std::fs;
use std::path::Path;

/// An opened `.pak` archive.
pub struct Pak {
    data: Vec<u8>,
    entries: Vec<(String, u32, u32)>, // (name, filepos, filelen)
}

impl Pak {
    /// Open and index a `.pak` file.
    pub fn open(path: &Path) -> Result<Self, String> {
        let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if data.len() < 12 || &data[0..4] != b"PACK" {
            return Err(format!("{}: not a pak (bad magic)", path.display()));
        }
        let dirofs = i32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let dirlen = i32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

        let mut entries = Vec::new();
        let end = dirofs.saturating_add(dirlen).min(data.len());
        let mut p = dirofs;
        while p + 64 <= end {
            let name = cstr(&data[p..p + 56]);
            let filepos =
                i32::from_le_bytes([data[p + 56], data[p + 57], data[p + 58], data[p + 59]]) as u32;
            let filelen =
                i32::from_le_bytes([data[p + 60], data[p + 61], data[p + 62], data[p + 63]]) as u32;
            entries.push((name, filepos, filelen));
            p += 64;
        }
        Ok(Self { data, entries })
    }

    /// Borrow a file's bytes by exact name (e.g. `"maps/q2dm1.bsp"`), case-insensitively.
    pub fn read(&self, name: &str) -> Option<&[u8]> {
        let want = name.to_ascii_lowercase();
        self.entries
            .iter()
            .find(|(n, _, _)| n.to_ascii_lowercase() == want)
            .and_then(|(_, off, len)| {
                let s = *off as usize;
                let e = s.checked_add(*len as usize)?;
                (e <= self.data.len()).then_some(&self.data[s..e])
            })
    }

    /// Iterate entry names (for debugging / listing).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _, _)| n.as_str())
    }
}

/// Read a NUL-terminated C string from the front of `b` (lossy UTF-8).
fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory pak with one entry, for round-trip testing.
    fn one_entry_pak(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        // header: magic + placeholder dirofs/dirlen (fixed up after the dir is appended)
        buf.extend_from_slice(b"PACK");
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        // payload at offset 12 (right after the header)
        let filepos = 12i32;
        buf.extend_from_slice(payload);
        // directory entry: name[56] + filepos + filelen
        let dirofs = buf.len() as i32;
        let mut entry = [0u8; 64];
        let nb = name.as_bytes();
        entry[..nb.len()].copy_from_slice(nb);
        entry[56..60].copy_from_slice(&filepos.to_le_bytes());
        entry[60..64].copy_from_slice(&(payload.len() as i32).to_le_bytes());
        buf.extend_from_slice(&entry);
        // fix up the header's dirofs/dirlen
        buf[4..8].copy_from_slice(&dirofs.to_le_bytes());
        buf[8..12].copy_from_slice(&64i32.to_le_bytes()); // one 64-byte entry
        buf
    }

    #[test]
    fn reads_file_from_pak() {
        // write to a temp file (Pak::open takes a path)
        let dir = std::env::temp_dir().join("qbots_pak_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.pak");
        std::fs::write(&path, one_entry_pak("maps/q2dm1.bsp", b"IBSPDATA")).unwrap();
        let pak = Pak::open(&path).unwrap();
        assert_eq!(pak.read("maps/q2dm1.bsp").unwrap(), b"IBSPDATA");
        assert_eq!(pak.read("maps/missing.bsp"), None);
        // case-insensitive
        assert_eq!(pak.read("MAPS/Q2DM1.BSP").unwrap(), b"IBSPDATA");
    }
}
