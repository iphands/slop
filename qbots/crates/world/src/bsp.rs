//! Q2 BSP loader (IBSP, version 38; structs in `files.h:294+`).
//!
//! Parses the header + the collision-relevant lumps (planes/nodes/leafs/brushes/
//! brushsides/leafbrushes/models) into typed arrays. Visibility/areas/nav land in T2–T4.

use std::collections::HashMap;
use std::path::Path;

use q2proto::{DecodeError, Reader};

use crate::pak::Pak;

pub const BSP_VERSION: i32 = 38;
/// `HEADER_LUMPS` (`files.h:292`).
pub const NUM_LUMPS: usize = 19;

// Lump indices (`files.h:273`).
const LUMP_ENTITIES: usize = 0;
const LUMP_PLANES: usize = 1;
const LUMP_VISIBILITY: usize = 3;
const LUMP_NODES: usize = 4;
const LUMP_LEAFS: usize = 8;
const LUMP_LEAFBRUSHES: usize = 10;
const LUMP_MODELS: usize = 13;
const LUMP_BRUSHES: usize = 14;
const LUMP_BRUSHSIDES: usize = 15;

/// DM spawn classname (`g_spawn.c`).
const SPAWN_DEATHMATCH: &str = "info_player_deathmatch";
/// Single-player start, used as a spawn fallback on maps short on DM spawns.
const SPAWN_START: &str = "info_player_start";

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

/// A parsed map entity from `LUMP_ENTITIES` — the text block of
/// `{ "classname" "..." "origin" "x y z" ... }` entries (`g_spawn.c:G_ParseEntity`).
/// `classname` is mirrored out of `fields` for convenience filtering.
#[derive(Debug, Clone)]
pub struct BspEntity {
    pub classname: String,
    /// Every quoted `"key" "value"` pair in the entity (classname included).
    pub fields: HashMap<String, String>,
}

impl BspEntity {
    /// Parse the `"origin" "x y z"` triplet, if present and well-formed.
    pub fn origin(&self) -> Option<[f32; 3]> {
        let v = self.fields.get("origin")?;
        let mut it = v.split_ascii_whitespace();
        let x = it.next()?.parse::<f32>().ok()?;
        let y = it.next()?.parse::<f32>().ok()?;
        let z = it.next()?.parse::<f32>().ok()?;
        Some([x, y, z])
    }

    /// Parse the `"angle" "deg"` yaw, if present.
    pub fn angle(&self) -> Option<f32> {
        self.fields.get("angle")?.trim().parse::<f32>().ok()
    }
}

/// A deathmatch spawn point (`info_player_deathmatch`): origin + facing yaw.
#[derive(Debug, Clone, Copy)]
pub struct SpawnPoint {
    pub origin: [f32; 3],
    pub angle: Option<f32>,
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
    /// Parsed `LUMP_ENTITIES` text block → map entities (spawns, items, weapons).
    pub entities: Vec<BspEntity>,
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
        // `LUMP_ENTITIES` is a NUL-terminated text block; missing/empty → no entities.
        let entities = parse_entities(slice(LUMP_ENTITIES).unwrap_or(&[]));

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
            entities,
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

    /// All entities whose `classname` matches, in file order.
    pub fn find_class(&self, classname: &str) -> Vec<&BspEntity> {
        self.entities
            .iter()
            .filter(|e| e.classname == classname)
            .collect()
    }

    /// DM spawn points (`info_player_deathmatch`), falling back to
    /// `info_player_start` when a map has no DM spawns. Only entities with a
    /// parseable origin count.
    pub fn spawn_points(&self) -> Vec<SpawnPoint> {
        let mut out = collect_spawns(self, SPAWN_DEATHMATCH);
        if out.is_empty() {
            out = collect_spawns(self, SPAWN_START);
        }
        out
    }
}

fn collect_spawns(bsp: &Bsp, classname: &str) -> Vec<SpawnPoint> {
    bsp.find_class(classname)
        .iter()
        .filter_map(|e| {
            e.origin().map(|origin| SpawnPoint {
                origin,
                angle: e.angle(),
            })
        })
        .collect()
}

/// Parse the `LUMP_ENTITIES` text block into entities. The format is a sequence
/// of `{ "key" "value" ... }` groups (`g_spawn.c:G_ParseEntity`, tokenized like
/// `COM_Parse`). Unknown/garbage bytes between groups are skipped.
fn parse_entities(raw: &[u8]) -> Vec<BspEntity> {
    let text = std::str::from_utf8(raw).unwrap_or("");
    let tokens = tokenize_entities(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "{" {
            i += 1;
            let mut fields = HashMap::new();
            while i < tokens.len() && tokens[i] != "}" {
                let key = tokens[i].clone();
                i += 1;
                // A value is the next token that isn't a brace; a key with no
                // value (e.g. a stray `"classname"`) is dropped, matching Q2.
                if i < tokens.len() && tokens[i] != "{" && tokens[i] != "}" {
                    fields.insert(key, tokens[i].clone());
                    i += 1;
                }
            }
            if i < tokens.len() {
                i += 1; // consume '}'
            }
            let classname = fields.get("classname").cloned().unwrap_or_default();
            out.push(BspEntity { classname, fields });
        } else {
            i += 1;
        }
    }
    out
}

/// Tokenize entity text into `{`, `}`, and the contents of quoted strings.
/// Mirrors `COM_Parse`'s string handling for the keys/values we care about.
fn tokenize_entities(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'{' || c == b'}' {
            toks.push((c as char).to_string());
            i += 1;
        } else if c == b'"' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let val = std::str::from_utf8(&bytes[start..i])
                .unwrap_or("")
                .to_string();
            toks.push(val);
            if i < bytes.len() {
                i += 1; // closing quote
            }
        } else {
            // Whitespace / comments / stray bytes between tokens — skip.
            i += 1;
        }
    }
    toks
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
        let mut mins = read_f3(&mut r)?;
        let mut maxs = read_f3(&mut r)?;
        r.skip(12)?; // origin
        let headnode = r.read_i32()?;
        r.skip(8)?; // firstface + numface

        // Apply the same -1/+1 margin as yquake2 (collision.c:1220-1223)
        // "spread the mins / maxs by a pixel" for collision tolerance
        mins[0] -= 1.0;
        mins[1] -= 1.0;
        mins[2] -= 1.0;
        maxs[0] += 1.0;
        maxs[1] += 1.0;
        maxs[2] += 1.0;

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

    #[test]
    fn parse_entities_round_trips_spawns_and_weapons() {
        let block = br#"
        {
        "classname" "info_player_deathmatch"
        "origin" "512 -128 24"
        "angle" "90"
        }
        {
        "classname" "info_player_deathmatch"
        "origin" "-256 0 24"
        }
        {
        "classname" "weapon_rocketlauncher"
        "origin" "0 1024 40"
        "spawnflags" "1"
        }
        "#;
        let ents = parse_entities(block);
        assert_eq!(ents.len(), 3);

        // find_class + origin/angle helpers.
        let spawns = ents
            .iter()
            .filter(|e| e.classname == "info_player_deathmatch")
            .collect::<Vec<_>>();
        assert_eq!(spawns.len(), 2);
        assert_eq!(spawns[0].origin(), Some([512.0, -128.0, 24.0]));
        assert_eq!(spawns[0].angle(), Some(90.0));
        // second spawn has no angle.
        assert_eq!(spawns[1].angle(), None);

        let rl = ents
            .iter()
            .find(|e| e.classname == "weapon_rocketlauncher")
            .expect("RL entity present");
        assert_eq!(rl.origin(), Some([0.0, 1024.0, 40.0]));
        assert_eq!(rl.fields.get("spawnflags").map(String::as_str), Some("1"));
    }

    #[test]
    fn parse_entities_ignores_garbage_between_groups() {
        // Stray text / a key with no value must not corrupt parsing.
        let block = b"junk { \"classname\" \"light\" \"origin\" \"1 2 3\" } more junk { \"classname\" \"misc_teleporter\" }";
        let ents = parse_entities(block);
        assert_eq!(ents.len(), 2);
        assert_eq!(ents[0].classname, "light");
        assert_eq!(ents[1].classname, "misc_teleporter");
    }

    /// A real BSP's entities round-trip through `Bsp::from_bytes` (via the
    /// entities lump slot) and the `spawn_points()` / `find_class()` helpers.
    #[test]
    fn bsp_exposes_spawn_points_and_find_class() {
        let mut buf = minimal_bsp();
        // minimal_bsp never writes LUMP_ENTITIES; overwrite its slot with a real
        // entities block so `from_bytes` parses it.
        let block = br#"{
"classname" "info_player_deathmatch"
"origin" "10 20 30"
}
{
"classname" "weapon_rocketlauncher"
"origin" "100 200 300"
}"#;
        let lump_pos = 8; // magic(4)+version(4) → first lump entry
        let ofs = buf.len() as i32;
        buf.extend_from_slice(block);
        let base = lump_pos + LUMP_ENTITIES * 8;
        buf[base..base + 4].copy_from_slice(&ofs.to_le_bytes());
        buf[base + 4..base + 8].copy_from_slice(&(block.len() as i32).to_le_bytes());

        let bsp = Bsp::from_bytes(&buf).unwrap();
        let spawns = bsp.spawn_points();
        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].origin, [10.0, 20.0, 30.0]);

        let rl = bsp.find_class("weapon_rocketlauncher");
        assert_eq!(rl.len(), 1);
        assert_eq!(rl[0].origin(), Some([100.0, 200.0, 300.0]));
    }

    #[test]
    fn spawn_points_fall_back_to_info_player_start() {
        let block = br#"
        { "classname" "info_player_start" "origin" "5 6 7" }
        { "classname" "weapon_shotgun" "origin" "1 1 1" }
        "#;
        let ents = parse_entities(block);
        // Mirror spawn_points() logic on a parsed vec: no DM spawns → use start.
        let has_dm = ents.iter().any(|e| e.classname == "info_player_deathmatch");
        assert!(!has_dm);
        let starts = ents
            .iter()
            .filter(|e| e.classname == "info_player_start")
            .collect::<Vec<_>>();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].origin(), Some([5.0, 6.0, 7.0]));
    }

    #[test]
    fn model_bounds_have_margin() {
        // Build a minimal BSP with one model to verify the -1/+1 margin is applied.
        let mut buf = minimal_bsp();

        // Add a LUMP_MODELS with one model: mins=[0,0,0], maxs=[100,100,100], origin=[0,0,0], headnode=0
        let mut model = Vec::new();
        model.extend_from_slice(&[0f32, 0.0, 0.0].map(|f| f.to_le_bytes()).concat()); // mins
        model.extend_from_slice(&[100f32, 100.0, 100.0].map(|f| f.to_le_bytes()).concat()); // maxs
        model.extend_from_slice(&[0f32, 0.0, 0.0].map(|f| f.to_le_bytes()).concat()); // origin
        model.extend_from_slice(&0i32.to_le_bytes()); // headnode
        model.extend_from_slice(&0i32.to_le_bytes()); // firstface
        model.extend_from_slice(&0i32.to_le_bytes()); // numfaces

        let lump_pos = 8; // magic(4)+version(4) → first lump entry
        let ofs = buf.len() as i32;
        buf.extend_from_slice(&model);
        let base = lump_pos + LUMP_MODELS * 8;
        buf[base..base + 4].copy_from_slice(&ofs.to_le_bytes());
        buf[base + 4..base + 8].copy_from_slice(&(model.len() as i32).to_le_bytes());

        let bsp = Bsp::from_bytes(&buf).unwrap();
        assert_eq!(bsp.models.len(), 1);

        // Verify the -1/+1 margin is applied (collision.c:1220-1223)
        let m = &bsp.models[0];
        assert_eq!(m.mins, [-1.0, -1.0, -1.0], "mins should be raw - 1");
        assert_eq!(m.maxs, [101.0, 101.0, 101.0], "maxs should be raw + 1");
    }
}
