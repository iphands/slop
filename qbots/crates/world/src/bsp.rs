//! Q2 BSP loader (IBSP, version 38; structs in `files.h:294+`).
//!
//! Parses the header + the collision-relevant lumps (planes/nodes/leafs/brushes/
//! brushsides/leafbrushes/models) into typed arrays. Visibility/areas/nav land in T2–T4.

use std::path::Path;

use q2proto::{DecodeError, Reader};

use crate::pak::Pak;

pub const BSP_VERSION: i32 = 38;
/// `HEADER_LUMPS` (`files.h:292`).
pub const NUM_LUMPS: usize = 19;

// Lump indices (`files.h:273`).
const LUMP_PLANES: usize = 1;
const LUMP_VISIBILITY: usize = 3;
const LUMP_NODES: usize = 4;
const LUMP_LEAFS: usize = 8;
const LUMP_LEAFBRUSHES: usize = 10;
const LUMP_MODELS: usize = 13;
const LUMP_BRUSHES: usize = 14;
const LUMP_BRUSHSIDES: usize = 15;

#[derive(Debug, Clone, Copy)]
pub struct Lump {
    pub ofs: i32,
    pub len: i32,
}

#[derive(Debug, Clone)]
pub struct Header {
    pub version: i32,
    pub lumps: [Lump; NUM_LUMPS],
}

/// `dplane_t` (`files.h:327`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
    /// PLANE_X..PLANE_ANYZ (`files.h:316`).
    pub typ: i32,
}

/// `dnode_t` (`files.h:381`) — faces dropped (renderer-only).
#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub planenum: i32,
    /// `[node, leaf]`; leafs encoded as `-(leaf+1)`.
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
}

/// `dleaf_t` (`files.h:423`) — leaffaces dropped (renderer-only).
#[derive(Debug, Clone, Copy)]
pub struct Leaf {
    pub contents: i32,
    pub cluster: i16,
    pub area: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub firstleafbrush: u16,
    pub numleafbrushes: u16,
}

/// `dbrushside_t` (`files.h:440`) — texinfo dropped.
#[derive(Debug, Clone, Copy)]
pub struct BrushSide {
    pub planenum: u16,
}

/// `dbrush_t` (`files.h:446`).
#[derive(Debug, Clone, Copy)]
pub struct Brush {
    pub firstside: i32,
    pub numsides: i32,
    pub contents: i32,
}

/// `dmodel_t` (`files.h:301`) — faces dropped.
#[derive(Debug, Clone, Copy)]
pub struct Model {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub headnode: i32,
}

/// A parsed BSP — header + the collision structures.
pub struct Bsp {
    pub version: i32,
    pub planes: Vec<Plane>,
    pub nodes: Vec<Node>,
    pub leafs: Vec<Leaf>,
    pub brushes: Vec<Brush>,
    pub brushsides: Vec<BrushSide>,
    pub leafbrushes: Vec<u16>,
    pub models: Vec<Model>,
    /// Raw visibility (PVS) lump: `dvis_t` header + RLE-compressed bitvectors.
    pub vis: Vec<u8>,
}

impl Bsp {
    /// Parse BSP bytes (the full file contents).
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let header_min = 4 + 4 + NUM_LUMPS * 8;
        if data.len() < header_min {
            return Err("bsp too small for header".into());
        }
        if &data[0..4] != b"IBSP" {
            return Err(format!("not a bsp (magic {:?}, want IBSP)", &data[0..4]));
        }
        let version = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != BSP_VERSION {
            return Err(format!(
                "unsupported bsp version {version} (want {BSP_VERSION})"
            ));
        }

        let mut lumps = [Lump { ofs: 0, len: 0 }; NUM_LUMPS];
        let mut p = 8;
        for l in lumps.iter_mut() {
            l.ofs = i32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]);
            l.len = i32::from_le_bytes([data[p + 4], data[p + 5], data[p + 6], data[p + 7]]);
            p += 8;
        }

        let slice = |i: usize| -> Result<&[u8], String> {
            let l = &lumps[i];
            let s = l.ofs as usize;
            let e = s
                .checked_add(l.len as usize)
                .ok_or("lump offset overflow")?;
            if e > data.len() {
                return Err(format!("lump {i} out of range ({s}..{e} > {})", data.len()));
            }
            Ok(&data[s..e])
        };

        // Codec parsers return DecodeError; lift to String at each call site.
        let planes = parse_planes(slice(LUMP_PLANES)?).map_err(|e| e.to_string())?;
        let nodes = parse_nodes(slice(LUMP_NODES)?).map_err(|e| e.to_string())?;
        let leafs = parse_leafs(slice(LUMP_LEAFS)?).map_err(|e| e.to_string())?;
        let brushes = parse_brushes(slice(LUMP_BRUSHES)?).map_err(|e| e.to_string())?;
        let brushsides = parse_brushsides(slice(LUMP_BRUSHSIDES)?).map_err(|e| e.to_string())?;
        let leafbrushes = parse_leafbrushes(slice(LUMP_LEAFBRUSHES)?).map_err(|e| e.to_string())?;
        let models = parse_models(slice(LUMP_MODELS)?).map_err(|e| e.to_string())?;
        let vis = slice(LUMP_VISIBILITY).unwrap_or(&[]).to_vec();

        Ok(Self {
            version,
            planes,
            nodes,
            leafs,
            brushes,
            brushsides,
            leafbrushes,
            models,
            vis,
        })
    }

    /// Locate `<baseq2>/maps/<map>.bsp` — loose, or inside `pak*.pak` — and parse it.
    pub fn load(baseq2: &Path, map: &str) -> Result<Self, String> {
        let name = format!("maps/{map}.bsp");
        let loose = baseq2.join(&name);
        if loose.exists() {
            let data =
                std::fs::read(&loose).map_err(|e| format!("read {}: {e}", loose.display()))?;
            return Self::from_bytes(&data);
        }
        // Stock DM maps live in pak1; search pak0..pak9 ascending.
        for n in 0..=9 {
            let pakpath = baseq2.join(format!("pak{n}.pak"));
            if !pakpath.exists() {
                continue;
            }
            let pak = Pak::open(&pakpath)?;
            if let Some(data) = pak.read(&name) {
                return Self::from_bytes(data);
            }
        }
        Err(format!(
            "map '{map}' not found loose or in any pak under {}",
            baseq2.display()
        ))
    }
}

// ---- lump parsers (field-by-field via the codec; counts follow from lump size) ----

fn parse_planes(buf: &[u8]) -> Result<Vec<Plane>, DecodeError> {
    const SIZE: usize = 20; // 3*float + float + int
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        out.push(Plane {
            normal: read_f3(&mut r)?,
            dist: r.read_f32()?,
            typ: r.read_i32()?,
        });
    }
    Ok(out)
}

fn parse_nodes(buf: &[u8]) -> Result<Vec<Node>, DecodeError> {
    const SIZE: usize = 28; // planenum(4)+children(8)+mins(6)+maxs(6)+faces(4)
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        let planenum = r.read_i32()?;
        let children = [r.read_i32()?, r.read_i32()?];
        let mins = read_i16_3(&mut r)?;
        let maxs = read_i16_3(&mut r)?;
        r.skip(4)?; // firstface + numface (u16 each)
        out.push(Node {
            planenum,
            children,
            mins,
            maxs,
        });
    }
    Ok(out)
}

fn parse_leafs(buf: &[u8]) -> Result<Vec<Leaf>, DecodeError> {
    const SIZE: usize = 28;
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        let contents = r.read_i32()?;
        let cluster = r.read_i16()?;
        let area = r.read_i16()?;
        let mins = read_i16_3(&mut r)?;
        let maxs = read_i16_3(&mut r)?;
        r.skip(4)?; // firstleafface + numleaffaces (u16 each)
                    // u16 fields read as i16 then bit-reinterpreted (Reader has no read_u16).
        let firstleafbrush = r.read_i16()? as u16;
        let numleafbrushes = r.read_i16()? as u16;
        out.push(Leaf {
            contents,
            cluster,
            area,
            mins,
            maxs,
            firstleafbrush,
            numleafbrushes,
        });
    }
    Ok(out)
}

fn parse_brushes(buf: &[u8]) -> Result<Vec<Brush>, DecodeError> {
    const SIZE: usize = 12;
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        out.push(Brush {
            firstside: r.read_i32()?,
            numsides: r.read_i32()?,
            contents: r.read_i32()?,
        });
    }
    Ok(out)
}

fn parse_brushsides(buf: &[u8]) -> Result<Vec<BrushSide>, DecodeError> {
    const SIZE: usize = 4; // planenum(u16) + texinfo(i16)
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        let planenum = r.read_i16()? as u16;
        r.skip(2)?; // texinfo
        out.push(BrushSide { planenum });
    }
    Ok(out)
}

fn parse_leafbrushes(buf: &[u8]) -> Result<Vec<u16>, DecodeError> {
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / 2);
    while r.remaining() >= 2 {
        out.push(r.read_i16()? as u16);
    }
    Ok(out)
}

fn parse_models(buf: &[u8]) -> Result<Vec<Model>, DecodeError> {
    const SIZE: usize = 48; // mins(12)+maxs(12)+origin(12)+headnode(4)+faces(8)
    let mut r = Reader::new(buf);
    let mut out = Vec::with_capacity(buf.len() / SIZE);
    while r.remaining() >= SIZE {
        let mins = read_f3(&mut r)?;
        let maxs = read_f3(&mut r)?;
        r.skip(12)?; // origin
        let headnode = r.read_i32()?;
        r.skip(8)?; // firstface + numface
        out.push(Model {
            mins,
            maxs,
            headnode,
        });
    }
    Ok(out)
}

fn read_f3(r: &mut Reader) -> Result<[f32; 3], DecodeError> {
    Ok([r.read_f32()?, r.read_f32()?, r.read_f32()?])
}

fn read_i16_3(r: &mut Reader) -> Result<[i16; 3], DecodeError> {
    Ok([r.read_i16()?, r.read_i16()?, r.read_i16()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal IBSP (version 38) with the 7 parsed lumps populated for round-trips.
    fn minimal_bsp() -> Vec<u8> {
        let mut buf = b"IBSP".to_vec();
        buf.extend_from_slice(&38i32.to_le_bytes()); // version
                                                     // reserve 19 lumps × 8 bytes (filled below)
        let lump_pos = buf.len();
        buf.resize(buf.len() + NUM_LUMPS * 8, 0);

        let put = |idx: usize, bytes: &[u8], buf: &mut Vec<u8>| {
            let ofs = buf.len();
            buf.extend_from_slice(bytes);
            let base = lump_pos + idx * 8;
            buf[base..base + 4].copy_from_slice(&(ofs as i32).to_le_bytes());
            buf[base + 4..base + 8].copy_from_slice(&(bytes.len() as i32).to_le_bytes());
        };

        // one plane: normal (0,0,1), dist 16, type PLANE_Z(2)
        let mut plane = Vec::new();
        plane.extend_from_slice(&[0f32, 0.0, 1.0].map(|f| f.to_le_bytes()).concat());
        plane.extend_from_slice(&16f32.to_le_bytes());
        plane.extend_from_slice(&2i32.to_le_bytes());
        put(LUMP_PLANES, &plane, &mut buf);

        // one node: planenum 0, children [-1(leaf0), 1(node)], bounds
        let mut node = Vec::new();
        node.extend_from_slice(&0i32.to_le_bytes());
        node.extend_from_slice(&(-1i32).to_le_bytes());
        node.extend_from_slice(&1i32.to_le_bytes());
        node.extend_from_slice(&[-100i16, -100, -100].map(|v| v.to_le_bytes()).concat());
        node.extend_from_slice(&[100i16, 100, 100].map(|v| v.to_le_bytes()).concat());
        node.extend_from_slice(&0u16.to_le_bytes());
        node.extend_from_slice(&0u16.to_le_bytes());
        put(LUMP_NODES, &node, &mut buf);

        // one leaf: contents SOLID(1), cluster -1, area 0, brushes 0/0
        let mut leaf = Vec::new();
        leaf.extend_from_slice(&1i32.to_le_bytes());
        leaf.extend_from_slice(&(-1i16).to_le_bytes());
        leaf.extend_from_slice(&0i16.to_le_bytes());
        leaf.extend_from_slice(&[0i16, 0, 0].map(|v| v.to_le_bytes()).concat());
        leaf.extend_from_slice(&[0i16, 0, 0].map(|v| v.to_le_bytes()).concat());
        leaf.extend_from_slice(&0u16.to_le_bytes());
        leaf.extend_from_slice(&0u16.to_le_bytes());
        leaf.extend_from_slice(&0u16.to_le_bytes());
        leaf.extend_from_slice(&0u16.to_le_bytes());
        put(LUMP_LEAFS, &leaf, &mut buf);

        // one brush: firstside 0, numsides 6, contents SOLID
        let mut brush = Vec::new();
        brush.extend_from_slice(&0i32.to_le_bytes());
        brush.extend_from_slice(&6i32.to_le_bytes());
        brush.extend_from_slice(&1i32.to_le_bytes());
        put(LUMP_BRUSHES, &brush, &mut buf);

        buf
    }

    #[test]
    fn parses_minimal_bsp() {
        let bsp = Bsp::from_bytes(&minimal_bsp()).unwrap();
        assert_eq!(bsp.version, 38);
        assert_eq!(bsp.planes.len(), 1);
        assert_eq!(bsp.planes[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(bsp.planes[0].dist, 16.0);
        assert_eq!(bsp.planes[0].typ, 2);
        assert_eq!(bsp.nodes.len(), 1);
        assert_eq!(bsp.nodes[0].children, [-1, 1]);
        assert_eq!(bsp.leafs.len(), 1);
        assert_eq!(bsp.leafs[0].contents, 1);
        assert_eq!(bsp.brushes.len(), 1);
        assert_eq!(bsp.brushes[0].numsides, 6);
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(Bsp::from_bytes(b"XXXXrest").is_err());
    }
}
