//! Integration tests for the Quake 3 brain (`--brain q3`, Plan 37 T7).
//!
//! Drives the brain through the **public** `trait Brain` API (via `build_brain`) over a local
//! stub `Navigator` + an "open" `CollisionModel`, with no server. Covers the things the inline
//! unit tests can't reach from outside the crate: the `build_brain(Quake3)` → status round-trip,
//! the reaction-time fire gate, and that the whole perceive→FSM→aim→fire pipeline actually pulls
//! the trigger on a visible enemy.

use brain::{build_brain, BotSkill, BrainConfig, BrainContext, BrainKind, NavGoal, Worldview};
use client::parse::ConfigStrings;
use glam::Vec3;
use q2proto::{EntityState, Frame};
use world::CollisionModel;

/// A minimal scripted navigator: returns a fixed look-ahead point and records the last goal.
#[derive(Default)]
struct StubNav {
    pursue: Option<Vec3>,
    last_goal: Option<NavGoal>,
}

impl brain::nav_mode::Navigator for StubNav {
    fn set_goal(&mut self, goal: NavGoal, _from: Vec3) {
        self.last_goal = Some(goal);
    }
    fn update(&mut self, _pos: Vec3, _cm: Option<&CollisionModel>) -> bool {
        false
    }
    fn pursue_target(&self, _from: Vec3) -> Option<Vec3> {
        self.pursue
    }
    fn pursue_target_safe(&self, _from: Vec3, _cm: &CollisionModel) -> Option<Vec3> {
        self.pursue
    }
    fn current_edge_is_jump(&self) -> bool {
        false
    }
    fn force_replan(&mut self) {}
    fn blacklist_waypoint_if_blocked(&mut self, _pos: Vec3, _cm: &CollisionModel) {}
}

/// An "open" collision world: a single half-space whose solid side is far below, so every trace
/// between play-height points is clear (LOS always holds).
fn open_world() -> CollisionModel {
    CollisionModel::half_space([0.0, 0.0, 1.0], -100_000.0)
}

/// A worldview with us holding `view_model` and one enemy at `+x`, facing us.
fn view_with_enemy(view_model: &str, ammo: i16) -> Worldview {
    let mut frame = Frame::default();
    frame.playerstate.gunindex = 1;
    frame.playerstate.stats[1] = 100; // health
    frame.playerstate.stats[3] = ammo; // STAT_AMMO
    frame.playerstate.stats[5] = 100; // armor
    frame.entities = vec![EntityState {
        number: 9,
        origin: [400.0, 0.0, 0.0],
        angles: [0.0, 180.0, 0.0], // facing −x toward us
        modelindex: 255,
        ..Default::default()
    }];
    let mut cs = ConfigStrings::default();
    cs.set(32 + 1, view_model); // CS_MODELS + gunindex
    Worldview::from_frame(&frame, &cs, 0)
}

#[test]
fn build_q3_roundtrips_to_seek_ltg() {
    let brain = build_brain(
        BrainKind::Quake3,
        BotSkill::new(9, brain::Personality::Balanced),
        BrainConfig::default(),
    );
    assert_eq!(brain.status(), "seek-ltg");
}

#[test]
fn q3_fires_at_a_visible_enemy_after_reaction() {
    // A high-skill q3 bot vs a railgun-armed enemy in the open → should enter Fight and, after
    // its reaction window, fire at least once over a couple of seconds.
    let mut brain = build_brain(
        BrainKind::Quake3,
        BotSkill::new(9, brain::Personality::Balanced),
        BrainConfig::default(),
    );
    let cm = open_world();
    let view = view_with_enemy("models/weapons/v_rail/tris.md2", 20);

    let mut fired = false;
    let mut fired_tick = None;
    for t in 0..40 {
        let mut nav = StubNav {
            pursue: Some(Vec3::new(400.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = brain.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: Some(&cm),
            dt: 0.1,
            ticks: t,
            goal_override: None,
        });
        if out.intent.attack {
            fired = true;
            fired_tick.get_or_insert(t);
        }
    }
    assert_eq!(brain.status(), "fight", "engaged the visible enemy");
    assert!(fired, "q3 bot fired at the visible enemy at least once");
    // Reaction gate: a skilled bot's reaction_time ≈ 0.3 s → no fire on the very first tick.
    assert!(
        fired_tick.unwrap() >= 2,
        "did not fire before the reaction delay (first fire at tick {:?})",
        fired_tick
    );
}

#[test]
fn q3_requests_a_better_weapon_against_a_ranged_enemy() {
    // Starting on the blaster, far enemy → the brain should request a stronger weapon (`use`).
    let mut brain = build_brain(
        BrainKind::Quake3,
        BotSkill::new(7, brain::Personality::Balanced),
        BrainConfig::default(),
    );
    let cm = open_world();
    // We "hold" a railgun on the wire but the brain tracks its own optimistic held weapon
    // (starts Blaster), so it will request a switch toward the best weapon for the range.
    let view = view_with_enemy("models/weapons/v_blast/tris.md2", 100);
    let mut requested = false;
    for t in 0..10 {
        let mut nav = StubNav {
            pursue: Some(Vec3::new(400.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = brain.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: Some(&cm),
            dt: 0.1,
            ticks: t,
            goal_override: None,
        });
        if out.weapon_request.is_some() {
            requested = true;
        }
    }
    assert!(requested, "q3 bot requested a weapon switch in combat");
}

#[test]
fn q3_roams_with_no_enemy() {
    let mut brain = build_brain(
        BrainKind::Quake3,
        BotSkill::new(5, brain::Personality::Balanced),
        BrainConfig::default(),
    );
    let cm = open_world();
    let view = Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0);
    let mut nav = StubNav {
        pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
        ..Default::default()
    };
    let out = brain.tick(BrainContext {
        view: &view,
        nav: Some(&mut nav),
        cm: Some(&cm),
        dt: 0.1,
        ticks: 1,
        goal_override: None,
    });
    assert_eq!(brain.status(), "seek-ltg");
    assert!(out.intent.forward > 0.0, "roams toward the look-ahead");
    assert!(!out.intent.attack, "no firing without an enemy");
}
