//! File-based competition rosters (Plan 69).
//!
//! `qbots competition --roster <file.yaml>` fields an explicit, hand-editable list of groups
//! instead of the CLI matrix — the way to run "the top 8 from yesterday's marathon." A run also
//! *emits* a ranked roster (see [`emit_ranked_yaml`]) you trim down for the next round.
//!
//! The YAML mirrors the scoreboard's own vocabulary — the same short codes the board prints are
//! valid tokens here (they're the clap `ValueEnum` names), so an emitted roster round-trips back
//! through [`Roster::into_specs`] unchanged:
//!
//! ```yaml
//! count: 2            # optional file-wide default per-group count (fallback 8)
//! groups:
//!   - brain: mai      # BrainKind token — short code (mai) or long alias (main)
//!     navmode: sg
//!   - brain: q3
//!     navmode: sg
//!     char: cam       # CharPreset — only valid with a q3 brain
//!   - brain: xon
//!     navmode: nm
//!     xonchar: shp    # XonCharPreset — only valid with a xon brain
//!     count: 4        # per-group override
//!     tag: shpkings   # optional custom scoreboard tag (default: auto <brain>_<mode>[_<char>])
//!     skin: female/athena   # optional
//! ```

use crate::supervisor::{
    brain_code, char_code, group_tag, mode_code, xon_char_code, GroupChar, GroupSpec, ModeScore,
};

/// The default per-group bot count when neither the group nor the file sets one — matches the
/// `--count` CLI default so a roster with bare groups behaves like `competition --count 8`.
const DEFAULT_COUNT: usize = 8;

/// Q2's `netname` buffer is 16 bytes (`game/player/client.c`), so a bot name `<tag>_<i>` must be
/// ≤ 15 chars. The roster validates the *widest* index (`count`) up front.
const MAX_NETNAME: usize = 15;

/// A parsed roster file. Deserialized from YAML; [`Self::into_specs`] validates and lowers it into
/// the `GroupSpec` list `run_competition` consumes.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roster {
    /// File-wide default per-group count; a group's own `count` overrides it.
    #[serde(default)]
    count: Option<usize>,
    /// The groups to field, in scoreboard order.
    groups: Vec<RosterGroup>,
}

/// One group entry in a roster file. All enum fields are strings parsed at [`Roster::into_specs`]
/// time (via `ValueEnum::from_str`), so a typo yields a pointed error rather than a serde failure.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RosterGroup {
    brain: String,
    navmode: String,
    #[serde(default)]
    char: Option<String>,
    #[serde(default)]
    xonchar: Option<String>,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    skin: Option<String>,
}

impl Roster {
    /// Read and parse a roster YAML file (mirrors [`crate::config::Config::load`]).
    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
        serde_yaml::from_str(&text).map_err(|e| format!("parse {path}: {e}"))
    }

    /// Validate + lower the roster into `GroupSpec`s. Returns a human-readable error (with the
    /// offending group's 1-based index) on any rule violation. See the module docs for the rules.
    pub fn into_specs(self) -> Result<Vec<GroupSpec>, String> {
        if self.groups.is_empty() {
            return Err("roster has no groups".to_string());
        }
        let mut specs = Vec::with_capacity(self.groups.len());
        let mut seen_tags: Vec<String> = Vec::new();
        for (idx, g) in self.groups.into_iter().enumerate() {
            let n = idx + 1; // 1-based for messages
            let spec = g
                .into_spec(self.count)
                .map_err(|e| format!("group {n}: {e}"))?;
            if seen_tags.contains(&spec.tag) {
                return Err(format!(
                    "group {n}: duplicate tag '{}' — each group needs a distinct tag (bot names \
                     would otherwise collide and merge on the scoreboard)",
                    spec.tag
                ));
            }
            seen_tags.push(spec.tag.clone());
            specs.push(spec);
        }
        Ok(specs)
    }
}

impl RosterGroup {
    /// Parse + validate one group into a `GroupSpec`. `file_count` is the roster-wide default.
    fn into_spec(self, file_count: Option<usize>) -> Result<GroupSpec, String> {
        use clap::ValueEnum;

        let brain = brain::BrainKind::from_str(&self.brain, true)
            .map_err(|_| format!("unknown brain '{}'", self.brain))?;
        if brain == brain::BrainKind::RunTester {
            return Err("'runtester' is a non-combat brain and cannot compete".to_string());
        }
        let mode = crate::NavMode::from_str(&self.navmode, true)
            .map_err(|_| format!("unknown navmode '{}'", self.navmode))?;

        // The char axis: `char` only for q3, `xonchar` only for xon, never both.
        if self.char.is_some() && self.xonchar.is_some() {
            return Err("set only one of char/xonchar".to_string());
        }
        let gc = match (&self.char, &self.xonchar) {
            (Some(c), None) => {
                if brain != brain::BrainKind::Quake3 {
                    return Err(format!("char '{c}' is only valid for the q3 brain"));
                }
                let cp = brain::CharPreset::from_str(c, true)
                    .map_err(|_| format!("unknown char '{c}'"))?;
                GroupChar::Q3(cp)
            }
            (None, Some(x)) => {
                if brain != brain::BrainKind::Xon {
                    return Err(format!("xonchar '{x}' is only valid for the xon brain"));
                }
                let xp = brain::XonCharPreset::from_str(x, true)
                    .map_err(|_| format!("unknown xonchar '{x}'"))?;
                GroupChar::Xon(xp)
            }
            (None, None) => GroupChar::None,
            (Some(_), Some(_)) => unreachable!("both-set case handled above"),
        };

        let count = self.count.or(file_count).unwrap_or(DEFAULT_COUNT);
        if count == 0 {
            return Err("count must be >= 1".to_string());
        }

        // Tag: an explicit one, else the auto `<brain>_<mode>[_<char>]`. Custom tags must be
        // non-empty, whitespace-free (Q2 netname / scoreboard grouping), and leave room for
        // `_<count>` inside the 15-char limit.
        let tag = match self.tag {
            Some(t) => {
                if t.is_empty() {
                    return Err("tag must not be empty".to_string());
                }
                if t.chars().any(|c| c.is_whitespace()) {
                    return Err(format!("tag '{t}' must not contain whitespace"));
                }
                t
            }
            None => group_tag(mode, brain, gc),
        };
        // Widest bot name is `<tag>_<count>` (largest index). `1 + digits(count)` is the suffix.
        let suffix = 1 + count.to_string().len();
        if tag.len() + suffix > MAX_NETNAME {
            return Err(format!(
                "tag '{tag}' + '_{count}' is {} chars, over Q2's {MAX_NETNAME}-char name limit",
                tag.len() + suffix
            ));
        }

        // Skin: explicit → the character's own skin → None (dispatch fills None via distinct_skins).
        let skin = self.skin.or_else(|| gc.skin());

        Ok(GroupSpec {
            mode,
            brain,
            gc,
            count,
            skin,
            tag,
        })
    }
}

/// Render a **ranked** roster YAML from a finished competition: the `ranked` scoreboard rows
/// (K/D order, from `mode_scoreboard`) zipped back to their `specs` by tag, each group preceded by
/// a `# rank N  kd=… kills=… deaths=…` comment. This is the file the operator trims down for the
/// next round — so it emits only the *competitive identity* (brain/navmode/char/count/custom tag),
/// **not** skins: the next run assigns fresh distinct skins. serde_yaml can't emit comments, so
/// this is a hand formatter; the round-trip test proves it re-parses through [`Roster::into_specs`].
pub fn emit_ranked_yaml(ranked: &[ModeScore], specs: &[GroupSpec]) -> String {
    let mut out = String::new();
    out.push_str("# competition roster — ranked by K/D. Delete groups you don't want, edit\n");
    out.push_str("# counts, then rerun:  qbots competition --roster <this file>\n");
    out.push_str("groups:\n");
    for (rank, row) in ranked.iter().enumerate() {
        let Some(spec) = specs.iter().find(|s| s.tag == row.tag) else {
            continue; // a scoreboard row with no spec can't happen, but never panic on a dump
        };
        out.push_str(&format!(
            "  # rank {}  kd={:.2} kills={} deaths={}\n",
            rank + 1,
            row.kd(),
            row.kills,
            row.deaths,
        ));
        out.push_str(&format!("  - brain: {}\n", brain_code(spec.brain)));
        out.push_str(&format!("    navmode: {}\n", mode_code(spec.mode)));
        match spec.gc {
            GroupChar::Q3(c) => out.push_str(&format!("    char: {}\n", char_code(c))),
            GroupChar::Xon(x) => out.push_str(&format!("    xonchar: {}\n", xon_char_code(x))),
            GroupChar::None => {}
        }
        out.push_str(&format!("    count: {}\n", spec.count));
        // Emit a custom tag only when it isn't the auto `<brain>_<mode>[_<char>]` (which the
        // loader would reconstruct anyway) — keeps the dump minimal and round-trip-clean.
        if spec.tag != group_tag(spec.mode, spec.brain, spec.gc) {
            out.push_str(&format!("    tag: {}\n", spec.tag));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs_from(yaml: &str) -> Result<Vec<GroupSpec>, String> {
        let r: Roster = serde_yaml::from_str(yaml).map_err(|e| e.to_string())?;
        r.into_specs()
    }

    #[test]
    fn parses_a_full_roster_with_all_axes() {
        let specs = specs_from(
            "count: 2\n\
             groups:\n\
             \x20 - brain: mai\n\
             \x20   navmode: sg\n\
             \x20 - brain: q3\n\
             \x20   navmode: sg\n\
             \x20   char: cam\n\
             \x20 - brain: xon\n\
             \x20   navmode: nm\n\
             \x20   xonchar: shp\n\
             \x20   count: 4\n\
             \x20   tag: shpkings\n\
             \x20   skin: female/athena\n",
        )
        .unwrap();
        assert_eq!(specs.len(), 3);
        // File-wide count applies to groups without their own.
        assert_eq!(specs[0].tag, "mai_sg");
        assert_eq!(specs[0].count, 2);
        assert_eq!(specs[1].tag, "q3_sg_cam");
        assert_eq!(specs[1].count, 2);
        // Per-group count + custom tag + explicit skin all take effect.
        assert_eq!(specs[2].tag, "shpkings");
        assert_eq!(specs[2].count, 4);
        assert_eq!(specs[2].skin.as_deref(), Some("female/athena"));
    }

    #[test]
    fn long_alias_tokens_parse_same_as_short_codes() {
        let specs = specs_from(
            "groups:\n\
             \x20 - brain: main\n\
             \x20   navmode: astar\n\
             \x20   count: 1\n",
        )
        .unwrap();
        assert_eq!(specs[0].tag, "mai_as");
        assert_eq!(specs[0].brain, brain::BrainKind::Main);
        assert_eq!(specs[0].mode, crate::NavMode::Astar);
    }

    #[test]
    fn default_count_is_eight_when_unset() {
        let specs = specs_from("groups:\n  - brain: mai\n    navmode: as\n").unwrap();
        assert_eq!(specs[0].count, 8);
    }

    #[test]
    fn char_skin_defaults_to_the_preset_skin() {
        let specs =
            specs_from("groups:\n  - brain: q3\n    navmode: as\n    char: gru\n    count: 1\n")
                .unwrap();
        assert_eq!(
            specs[0].skin.as_deref(),
            Some(brain::CharPreset::Grunt.skin())
        );
    }

    #[test]
    fn rejects_empty_groups() {
        let err = specs_from("groups: []\n").unwrap_err();
        assert!(err.contains("no groups"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_tokens() {
        assert!(specs_from("groups:\n  - brain: nope\n    navmode: as\n")
            .unwrap_err()
            .contains("unknown brain"));
        assert!(specs_from("groups:\n  - brain: mai\n    navmode: warp\n")
            .unwrap_err()
            .contains("unknown navmode"));
    }

    #[test]
    fn rejects_runtester() {
        let err = specs_from("groups:\n  - brain: run\n    navmode: as\n").unwrap_err();
        assert!(err.contains("runtester"), "got: {err}");
    }

    #[test]
    fn rejects_char_on_non_q3_brain() {
        let err =
            specs_from("groups:\n  - brain: mai\n    navmode: as\n    char: cam\n").unwrap_err();
        assert!(err.contains("only valid for the q3 brain"), "got: {err}");
    }

    #[test]
    fn rejects_xonchar_on_non_xon_brain() {
        let err =
            specs_from("groups:\n  - brain: q3\n    navmode: as\n    xonchar: shp\n").unwrap_err();
        assert!(err.contains("only valid for the xon brain"), "got: {err}");
    }

    #[test]
    fn rejects_both_char_and_xonchar() {
        let err = specs_from(
            "groups:\n  - brain: q3\n    navmode: as\n    char: cam\n    xonchar: shp\n",
        )
        .unwrap_err();
        assert!(err.contains("only one of char/xonchar"), "got: {err}");
    }

    #[test]
    fn rejects_zero_count() {
        let err =
            specs_from("groups:\n  - brain: mai\n    navmode: as\n    count: 0\n").unwrap_err();
        assert!(err.contains("count must be >= 1"), "got: {err}");
    }

    #[test]
    fn rejects_duplicate_resolved_tags() {
        // Two identical groups → identical auto tags → collision.
        let err = specs_from(
            "groups:\n  - brain: mai\n    navmode: as\n  - brain: mai\n    navmode: as\n",
        )
        .unwrap_err();
        assert!(err.contains("duplicate tag"), "got: {err}");
    }

    #[test]
    fn rejects_tag_too_long_for_netname() {
        // 13-char tag + '_100' (count 100 → 4-char suffix) = 17 > 15.
        let err = specs_from(
            "groups:\n  - brain: mai\n    navmode: as\n    tag: abcdefghijklm\n    count: 100\n",
        )
        .unwrap_err();
        assert!(err.contains("over Q2's"), "got: {err}");
    }

    #[test]
    fn rejects_tag_with_whitespace() {
        let err = specs_from(
            "groups:\n  - brain: mai\n    navmode: as\n    tag: \"bad tag\"\n    count: 1\n",
        )
        .unwrap_err();
        assert!(err.contains("whitespace"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_field() {
        // deny_unknown_fields makes a typo'd key fail loudly rather than be silently ignored.
        let err =
            specs_from("groups:\n  - brain: mai\n    navmode: as\n    braim: mai\n").unwrap_err();
        assert!(!err.is_empty());
    }

    fn score(tag: &str, kills: u64, deaths: u64, bots: usize) -> ModeScore {
        ModeScore {
            tag: tag.to_string(),
            kills,
            deaths,
            env_suicides: 0,
            health_picked: 0,
            armor_picked: 0,
            weapons_picked: 0,
            bots,
        }
    }

    #[test]
    fn emitted_roster_round_trips_and_is_ranked() {
        // Spec order is NOT rank order — the emit must follow the ranking, not the spec list.
        let specs = vec![
            GroupSpec {
                mode: crate::NavMode::HybridSegment,
                brain: brain::BrainKind::Main,
                gc: GroupChar::None,
                count: 2,
                skin: Some("male/random".into()),
                tag: "mai_sg".into(),
            },
            GroupSpec {
                mode: crate::NavMode::HybridSegment,
                brain: brain::BrainKind::Quake3,
                gc: GroupChar::Q3(brain::CharPreset::Camper),
                count: 2,
                skin: None,
                tag: "q3_sg_cam".into(),
            },
            GroupSpec {
                mode: crate::NavMode::Navmesh,
                brain: brain::BrainKind::Xon,
                gc: GroupChar::Xon(brain::XonCharPreset::Sharp),
                count: 4,
                skin: None,
                tag: "shpkings".into(), // custom tag ≠ auto xon_nm_shp
            },
        ];
        // Ranked by K/D: shpkings (2.0) > mai_sg (1.0) > q3_sg_cam (0.5).
        let ranked = vec![
            score("shpkings", 8, 4, 4),
            score("mai_sg", 2, 2, 2),
            score("q3_sg_cam", 1, 2, 2),
        ];

        let yaml = emit_ranked_yaml(&ranked, &specs);
        // Comments are present, in rank order, with the right stats.
        let r1 = yaml.find("# rank 1  kd=2.00 kills=8 deaths=4").unwrap();
        let r2 = yaml.find("# rank 2  kd=1.00 kills=2 deaths=2").unwrap();
        let r3 = yaml.find("# rank 3  kd=0.50 kills=1 deaths=2").unwrap();
        assert!(r1 < r2 && r2 < r3, "rank comments must be in order");
        // A custom tag is emitted; the auto tags are omitted (loader reconstructs them).
        assert!(yaml.contains("    tag: shpkings\n"));
        assert!(!yaml.contains("tag: mai_sg"));
        // No skins in the dump — the next run regenerates distinct ones.
        assert!(!yaml.contains("skin:"));

        // Re-parse → same competitive identity, now in RANK order.
        let reloaded = specs_from(&yaml).unwrap();
        assert_eq!(reloaded.len(), 3);
        let ident = |s: &GroupSpec| (s.mode, s.brain, s.gc, s.count, s.tag.clone());
        assert_eq!(ident(&reloaded[0]), ident(&specs[2])); // shpkings
        assert_eq!(ident(&reloaded[1]), ident(&specs[0])); // mai_sg
        assert_eq!(ident(&reloaded[2]), ident(&specs[1])); // q3_sg_cam
    }
}
