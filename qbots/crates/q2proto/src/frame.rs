//! `svc_frame` — a decoded server snapshot (our player state + world entities).
//!
//! Ports `CL_ParseFrame` (`cl_parse.c:739`): reads the frame header, `areabits`, the
//! `svc_playerinfo`+`player_state`, and the `svc_packetentities` merge loop
//! (`CL_ParsePacketEntities:363`). Folds Plan 04 T2 (ring + header) and T3 (snapshot).

use crate::entitystate::EntityState;
use crate::ops::{SvcOp, UPDATE_BACKUP, UPDATE_MASK, U_REMOVE};
use crate::playerstate::PlayerState;
use crate::{DecodeError, Reader};

/// One server snapshot. `entities` is the full visible set for this frame (merged).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub serverframe: i32,
    pub deltaframe: i32,
    pub valid: bool,
    pub playerstate: PlayerState,
    pub entities: Vec<EntityState>,
}

/// A ring of `UPDATE_BACKUP` frames for delta decoding, indexed `serverframe & UPDATE_MASK`.
#[derive(Debug, Clone)]
pub struct FrameRing {
    frames: Vec<Frame>,
}

impl FrameRing {
    pub fn new() -> Self {
        Self {
            frames: (0..UPDATE_BACKUP).map(|_| Frame::default()).collect(),
        }
    }

    /// Borrow the frame slot for `serverframe` (may be stale — check `.valid`/`.serverframe`).
    pub fn get(&self, serverframe: i32) -> &Frame {
        &self.frames[(serverframe as usize) & UPDATE_MASK]
    }

    /// Store a freshly parsed frame in its ring slot.
    pub fn store(&mut self, frame: Frame) {
        let idx = (frame.serverframe as usize) & UPDATE_MASK;
        self.frames[idx] = frame;
    }
}

impl Default for FrameRing {
    fn default() -> Self {
        Self::new()
    }
}

/// `CL_ParsePacketEntities(oldframe, newframe)`: decode the entity merge loop, terminated
/// by a zero entity number. `old` is the delta source's entity slice (or `None`).
pub fn parse_packet_entities(
    r: &mut Reader,
    old: Option<&[EntityState]>,
) -> Result<Vec<EntityState>, DecodeError> {
    let old_ents = old.unwrap_or(&[]);
    let mut out: Vec<EntityState> = Vec::new();
    let mut old_idx = 0usize;

    loop {
        let (newnum, bits) = EntityState::parse_bits(r)?;
        if newnum == 0 {
            break; // end sentinel
        }
        if newnum < 0 {
            return Err(DecodeError::Invalid("entity number"));
        }

        // Copy unchanged old entities (oldnum < newnum) straight through.
        while old_idx < old_ents.len() && old_ents[old_idx].number < newnum {
            out.push(old_ents[old_idx].clone());
            old_idx += 1;
        }

        // Delta source: the matching old entity, or a null baseline for a new entity.
        let from = if old_idx < old_ents.len() && old_ents[old_idx].number == newnum {
            let e = old_ents[old_idx].clone();
            old_idx += 1;
            e
        } else {
            EntityState::default()
        };

        if bits & U_REMOVE != 0 {
            // Removed from this frame — already consumed the old slot; emit nothing.
            continue;
        }

        out.push(EntityState::read_delta(r, &from, newnum, bits)?);
    }

    // Any trailing old entities are unchanged.
    while old_idx < old_ents.len() {
        out.push(old_ents[old_idx].clone());
        old_idx += 1;
    }

    Ok(out)
}

/// `CL_ParseFrame`: parse the frame body (the `svc_frame` opcode has been consumed)
/// using `ring` to resolve the delta source.
pub fn parse_frame(r: &mut Reader, ring: &FrameRing) -> Result<Frame, DecodeError> {
    let serverframe = r.read_i32()?;
    let deltaframe = r.read_i32()?;
    let _surpress_count = r.read_u8()?;

    // areabits: length byte + that many bytes.
    let areabits_len = r.read_u8()?;
    r.skip(areabits_len as usize)?;

    // Resolve the delta source (None → uncompressed baseline).
    let (old, valid) = match deltaframe <= 0 {
        true => (None, true),
        false => {
            let cand = ring.get(deltaframe);
            let ok = cand.valid && cand.serverframe == deltaframe;
            (ok.then_some(cand), ok)
        }
    };

    // svc_playerinfo + player_state
    let pi_op = r.read_u8()?;
    if SvcOp::from_u8(pi_op) != Some(SvcOp::Playerinfo) {
        return Err(DecodeError::Invalid("expected svc_playerinfo"));
    }
    let playerstate = PlayerState::read_delta(r, old.map(|f| &f.playerstate))?;

    // svc_packetentities + entity loop
    let pe_op = r.read_u8()?;
    if !matches!(
        SvcOp::from_u8(pe_op),
        Some(SvcOp::Packetentities) | Some(SvcOp::Deltapacketentities)
    ) {
        return Err(DecodeError::Invalid("expected svc_packetentities"));
    }
    let entities = parse_packet_entities(r, old.map(|f| f.entities.as_slice()))?;

    Ok(Frame {
        serverframe,
        deltaframe,
        valid,
        playerstate,
        entities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Writer;

    /// Build a minimal frame body: header + empty areabits + playerinfo(null ps) +
    /// packetentities(end sentinel only).
    fn minimal_frame_body(serverframe: i32, deltaframe: i32) -> Vec<u8> {
        let mut w = Writer::new();
        w.write_i32(serverframe);
        w.write_i32(deltaframe);
        w.write_u8(0); // surpressCount
        w.write_u8(0); // areabits len = 0
        w.write_u8(SvcOp::Playerinfo as u8);
        // player_state: flags=0 (no fields) + stats bitmask=0
        w.write_i16(0);
        w.write_i32(0);
        w.write_u8(SvcOp::Packetentities as u8);
        // terminator: server writes MSG_WriteShort(0) = 2 zero bytes (bits=0, number=0)
        w.write_u8(0);
        w.write_u8(0);
        w.freeze().to_vec()
    }

    #[test]
    fn parses_uncompressed_frame() {
        let body = minimal_frame_body(10, -1);
        let mut r = Reader::new(&body);
        let ring = FrameRing::new();
        let f = parse_frame(&mut r, &ring).unwrap();
        assert_eq!(f.serverframe, 10);
        assert!(f.valid); // deltaframe <= 0 → uncompressed
        assert!(f.entities.is_empty());
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn ring_store_and_delta_resolve() {
        let mut ring = FrameRing::new();
        // store a base frame at serverframe 5, then delta from it
        let base_body = minimal_frame_body(5, -1);
        let mut r = Reader::new(&base_body);
        let base = parse_frame(&mut r, &FrameRing::new()).unwrap();
        ring.store(base);

        // a delta frame referencing deltaframe 5
        let body = minimal_frame_body(6, 5);
        let mut r = Reader::new(&body);
        let f = parse_frame(&mut r, &ring).unwrap();
        assert_eq!(f.serverframe, 6);
        assert!(f.valid); // resolved delta from frame 5
    }

    #[test]
    fn stale_delta_is_invalid() {
        // deltaframe 99 was never stored → not valid, falls back to baseline.
        let body = minimal_frame_body(7, 99);
        let mut r = Reader::new(&body);
        let ring = FrameRing::new();
        let f = parse_frame(&mut r, &ring).unwrap();
        assert!(!f.valid);
    }
}
