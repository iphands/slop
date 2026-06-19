//! Moving-platform (`func_train`) ride execution (Plan 43).
//!
//! The nav graph (Plan 42) marks ride edges with [`world::RideInfo`] (board / far / dismount
//! world positions). This module turns "the current path edge is a ride" into the
//! approach → wait-for-platform → cross → dismount movement sequence the brain drives.
//!
//! The platform itself is a brush-model entity (`*N`), classified [`EntityClass::Unknown`]
//! by perception. We detect "the platform is here" by the presence of such a non-actor,
//! non-projectile entity near the board point — the only movers near a ride board point on
//! q2dm3 are the trains, so proximity is a reliable enough signal without resolving the
//! inline-model ↔ configstring mapping.

use glam::Vec3;
use world::RideInfo;

use crate::perception::{EntityClass, Worldview};

/// Horizontal distance (units) within which the bot is "at the board point".
const BOARD_NEAR: f32 = 48.0;
/// A platform counts as present if a (non-actor) entity is within this 3-D distance of the
/// platform's expected wire origin at the board corner ([`RideInfo::board_ent`]).
const PLATFORM_DETECT: f32 = 48.0;

/// What the bot should do this frame on a ride edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RidePhase {
    /// Walk to the board point (the platform's near path endpoint).
    Approach,
    /// At the board point but the platform isn't here yet — hold clear and wait.
    Wait,
    /// Platform present — cross to the far/dismount point (step on, ride, step off).
    Cross,
}

/// True if the train is at the board corner this frame (Plan 43). The `func_train` is a
/// brush-model entity (classified [`EntityClass::Unknown`]) whose **wire origin** equals
/// `path_corner - model.mins` — captured at build time in [`RideInfo::board_ent`]. We match
/// any non-actor, non-projectile entity within [`PLATFORM_DETECT`] of that expected origin.
pub fn platform_present(view: &Worldview, board_ent: Vec3) -> bool {
    view.entities().any(|e| {
        !matches!(
            e.class,
            EntityClass::SelfPlayer
                | EntityClass::EnemyPlayer
                | EntityClass::AllyPlayer
                | EntityClass::ProjectileRocket
                | EntityClass::ProjectileGrenade
        ) && (e.origin - board_ent).length() <= PLATFORM_DETECT
    })
}

/// Max distance (units) from the board↔far path within which a non-actor entity is taken to be
/// the train (for live-position tracking while carried).
const TRAIN_TRACK_MAX: f32 = 256.0;

/// The bot-origin position of the train's standable top **right now** (Plan 43 T7), derived from
/// the live entity origin. The brush's wire origin is `corner - mins` and its standable top-center
/// is `corner + [size.xy/2, size.z+24]`; their difference is a constant per train, recoverable as
/// `far - far_ent` (both stored in [`RideInfo`]). So `live_stand = entity.origin + (far - far_ent)`.
/// Picks the non-actor entity nearest the board↔far segment. The brain steers toward this while
/// carried so it stays centered on the moving platform instead of sliding off into the pit.
pub fn train_stand_now(view: &Worldview, info: &RideInfo) -> Option<Vec3> {
    let board_ent = Vec3::from(info.board_ent);
    let far_ent = Vec3::from(info.far_ent);
    let offset = Vec3::from(info.far) - far_ent;
    let mut best: Option<(f32, Vec3)> = None;
    for e in view.entities() {
        if matches!(
            e.class,
            EntityClass::SelfPlayer
                | EntityClass::EnemyPlayer
                | EntityClass::AllyPlayer
                | EntityClass::ProjectileRocket
                | EntityClass::ProjectileGrenade
        ) {
            continue;
        }
        let d = dist_point_segment(e.origin, board_ent, far_ent);
        if d <= TRAIN_TRACK_MAX && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, e.origin));
        }
    }
    best.map(|(_, o)| o + offset)
}

/// Distance from point `p` to segment `a`–`b`.
fn dist_point_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-6 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Decide the ride phase from the bot position, the ride info, and the worldview (Plan 43).
///
/// A **vertical lift** (`func_plat`/`func_door`) never waits: the bot walks onto the pad and
/// standing in the shaft is what summons + rides it, so it's Approach (until on the pad) then
/// Cross (target straight up → ~zero horizontal → the bot stands and is carried). A horizontal
/// **train** waits at the board point until the platform actually arrives.
pub fn ride_phase(pos: Vec3, info: &RideInfo, view: &Worldview) -> RidePhase {
    let board = Vec3::from(info.board);
    let horiz = (pos.truncate() - board.truncate()).length();
    if horiz > BOARD_NEAR {
        RidePhase::Approach
    } else if info.vertical || platform_present(view, Vec3::from(info.board_ent)) {
        RidePhase::Cross
    } else {
        RidePhase::Wait
    }
}

/// The world-space movement target for a ride phase: the board point while approaching or
/// waiting (hold position there), the dismount point while crossing. The caller turns this
/// into view-relative `forward`/`side` and zeroes movement on [`RidePhase::Wait`].
pub fn ride_target(phase: RidePhase, info: &RideInfo) -> Vec3 {
    match phase {
        RidePhase::Approach | RidePhase::Wait => Vec3::from(info.board),
        RidePhase::Cross => Vec3::from(info.dismount),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::Worldview;
    use client::parse::ConfigStrings;
    use q2proto::{EntityState, Frame};

    fn info() -> RideInfo {
        RideInfo {
            board: [100.0, 0.0, 50.0],
            far: [400.0, 0.0, 50.0],
            dismount: [430.0, 0.0, 50.0],
            model_index: 3,
            vertical: false,
            board_ent: [100.0, 0.0, 50.0],
            far_ent: [400.0, 0.0, 50.0],
            ladder: false,
        }
    }

    fn vinfo() -> RideInfo {
        RideInfo {
            vertical: true,
            ..info()
        }
    }

    /// A worldview with the given entity origins present (all classified Unknown — no model
    /// configstrings — which is exactly how a brush-model platform shows up).
    fn view_with(entity_origins: &[[f32; 3]]) -> Worldview {
        let mut frame = Frame::default();
        for (i, o) in entity_origins.iter().enumerate() {
            frame.entities.push(EntityState {
                number: 50 + i as i32,
                origin: *o,
                ..Default::default()
            });
        }
        Worldview::from_frame(&frame, &ConfigStrings::default(), 0)
    }

    #[test]
    fn far_from_board_is_approach() {
        let phase = ride_phase(Vec3::new(0.0, 0.0, 50.0), &info(), &view_with(&[]));
        assert_eq!(phase, RidePhase::Approach);
    }

    #[test]
    fn at_board_no_platform_is_wait() {
        let phase = ride_phase(Vec3::new(110.0, 0.0, 50.0), &info(), &view_with(&[]));
        assert_eq!(phase, RidePhase::Wait);
    }

    #[test]
    fn at_board_with_platform_is_cross() {
        // An entity sitting on the board point → platform present → cross.
        let phase = ride_phase(
            Vec3::new(110.0, 0.0, 50.0),
            &info(),
            &view_with(&[[100.0, 0.0, 50.0]]),
        );
        assert_eq!(phase, RidePhase::Cross);
    }

    #[test]
    fn vertical_lift_never_waits() {
        // On the pad with no detected platform entity: a vertical lift still crosses (rides up).
        let phase = ride_phase(Vec3::new(110.0, 0.0, 50.0), &vinfo(), &view_with(&[]));
        assert_eq!(phase, RidePhase::Cross);
    }

    #[test]
    fn ride_target_tracks_phase() {
        let i = info();
        assert_eq!(ride_target(RidePhase::Approach, &i), Vec3::from(i.board));
        assert_eq!(ride_target(RidePhase::Cross, &i), Vec3::from(i.dismount));
    }
}
