//! PVS — Potentially Visible Set ("can cluster A see cluster B?").
//!
//! Ports `CM_DecompressVis` + `CM_ClusterPVS` from `collision.c:1887/1936`. The vis lump
//! is a `dvis_t` header (`numclusters` + `bitofs[numclusters][2]`) followed by RLE-
//! compressed bitvectors. The server only sends entities in the viewer's PVS, so this
//! both explains what we see and gives a cheap line-of-sight pre-filter.

/// PVS over a parsed visibility lump.
pub struct Pvs {
    data: Vec<u8>, // the whole vis lump (dvis_t header + compressed bitvectors)
    numclusters: usize,
}

impl Pvs {
    /// Build from the raw visibility lump (`Bsp::vis`). `None` if the lump is empty.
    pub fn from_lump(data: Vec<u8>) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        let numclusters = i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        Some(Self { data, numclusters })
    }

    /// Total cluster count.
    pub fn numclusters(&self) -> usize {
        self.numclusters
    }

    /// `bitofs[cluster][DVIS_PVS]` — the byte offset of `cluster`'s PVS into the lump.
    /// The header is `[numclusters:i32][bitofs[numclusters][2]:i32]`, so `bitofs[c][0]`
    /// lives at byte `4 + c*8`.
    fn pvs_offset(&self, cluster: usize) -> Option<usize> {
        if cluster >= self.numclusters {
            return None;
        }
        let o = 4 + cluster * 8;
        if o + 4 > self.data.len() {
            return None;
        }
        Some(i32::from_le_bytes([
            self.data[o],
            self.data[o + 1],
            self.data[o + 2],
            self.data[o + 3],
        ]) as usize)
    }

    /// `CM_DecompressVis` — decompress `cluster`'s PVS into a `(numclusters+7)/8` bitset.
    /// Cluster `-1` (solid/void) → all zero (nothing visible). Missing data → all set.
    pub fn decompress(&self, cluster: i16) -> Vec<u8> {
        let row = self.numclusters.div_ceil(8);
        let mut out = vec![0u8; row];

        if cluster < 0 {
            return out; // void: nothing visible
        }
        let off = match self.pvs_offset(cluster as usize) {
            Some(o) if o < self.data.len() => o,
            _ => {
                // no vis info for this cluster → everything visible
                out.iter_mut().for_each(|b| *b = 0xff);
                return out;
            }
        };

        let mut ip = off;
        let mut op = 0usize;
        while op < row && ip < self.data.len() {
            let b = self.data[ip];
            if b != 0 {
                out[op] = b;
                op += 1;
                ip += 1;
            } else {
                ip += 1;
                if ip >= self.data.len() {
                    break;
                }
                let c = (self.data[ip] as usize).min(row - op);
                ip += 1;
                op += c; // bytes already zero
            }
        }
        out
    }

    /// Is cluster `to` potentially visible from cluster `from`?
    pub fn cluster_visible(&self, from: i16, to: i16) -> bool {
        if !(0..self.numclusters as i16).contains(&from) || to < 0 {
            return false;
        }
        let bits = self.decompress(from);
        let t = to as usize;
        t / 8 < bits.len() && bits[t / 8] & (1 << (t % 8)) != 0
    }

    /// How many clusters are visible from `cluster` (popcount of its PVS bitset).
    pub fn count_visible(&self, cluster: i16) -> usize {
        if cluster < 0 {
            return 0;
        }
        self.decompress(cluster)
            .iter()
            .map(|b| b.count_ones() as usize)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal vis lump: numclusters=16, with cluster 0 seeing clusters 0 and 3.
    fn mini_pvs() -> Vec<u8> {
        // header: numclusters(4) + bitofs[16][2] (16*8=128 bytes) = 132 bytes header
        let numclusters = 16usize;
        let mut buf = Vec::new();
        buf.extend_from_slice(&(numclusters as i32).to_le_bytes());
        // bitofs[c][0]: cluster 0 → offset 132; others → 0 (we'll set cluster 0 only)
        let pvs_offset = (numclusters * 8 + 4) as i32;
        for c in 0..numclusters {
            if c == 0 {
                buf.extend_from_slice(&pvs_offset.to_le_bytes()); // PVS offset
                buf.extend_from_slice(&0i32.to_le_bytes()); // PHS offset (unused)
            } else {
                buf.extend_from_slice(&0i32.to_le_bytes());
                buf.extend_from_slice(&0i32.to_le_bytes());
            }
        }
        // cluster 0's PVS bitset, RLE-encoded: byte 0x09 = bits 0 and 3 set; then 0x00 0x01 = one zero byte
        buf.push(0b00001001); // clusters 0 and 3
        buf.push(0x00); // run of zeros
        buf.push(0x01); // one zero byte
        buf
    }

    #[test]
    fn cluster_visibility() {
        let pvs = Pvs::from_lump(mini_pvs()).unwrap();
        assert_eq!(pvs.numclusters(), 16);
        let bits = pvs.decompress(0);
        assert_eq!(bits.len(), 2); // (16+7)/8
        assert!(bits[0] & (1 << 0) != 0); // cluster 0 sees itself
        assert!(bits[0] & (1 << 3) != 0); // cluster 0 sees cluster 3
        assert_eq!(bits[0] & (1 << 1), 0); // not cluster 1
        assert!(pvs.cluster_visible(0, 0));
        assert!(pvs.cluster_visible(0, 3));
        assert!(!pvs.cluster_visible(0, 1));
        assert_eq!(pvs.count_visible(0), 2);
    }

    #[test]
    fn void_cluster_sees_nothing() {
        let pvs = Pvs::from_lump(mini_pvs()).unwrap();
        assert_eq!(pvs.count_visible(-1), 0);
        assert!(!pvs.cluster_visible(-1, 0));
    }
}
