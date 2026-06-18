//! Player-skin selection for the fleet.
//!
//! The Q2 `skin` userinfo value is `model/skin` (e.g. `male/grunt`, `female/cobalt`);
//! other clients load that model+skin to render us. This module turns the `run --skin*`
//! CLI choice into a concrete per-bot `model/skin` string: a fixed skin (resolving a bare
//! name like `sniper` to its owning model), or a random skin per bot from a model's pool.
//!
//! Skins are discovered from the configured `baseq2/players/<model>/*.pcx` (so custom
//! skins are picked up), falling back to the canonical id Software set when the loose
//! player dir is absent (e.g. a pak-only install) — the value only needs to be a name the
//! *server's* clients can resolve, so the built-in list is a safe default.

use std::path::Path;

/// Canonical id Software male skins (`baseq2/pak0.pak` `players/male`).
const MALE_SKINS: &[&str] = &[
    "cipher", "claymore", "flak", "grunt", "howitzer", "major", "nightops", "pointman", "psycho",
    "rampage", "razor", "recon", "scout", "sniper", "viper",
];
/// Canonical id Software female skins (`baseq2/pak0.pak` `players/female`).
const FEMALE_SKINS: &[&str] = &[
    "athena", "brianna", "cobalt", "doomgal", "ensign", "jezebel", "jungle", "lotus", "stiletto",
    "venus", "voodoo",
];

/// A resolved skin choice for a fleet, built once from the CLI flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinSelection {
    /// No `--skin*` given — leave the userinfo default in place.
    Default,
    /// Every bot gets this exact `model/skin`.
    Fixed(String),
    /// Each bot draws a random `model/skin` from this pool.
    Random(Vec<String>),
}

impl SkinSelection {
    /// Build the selection from the mutually exclusive `run --skin*` flags. `skin` is the
    /// `--skin <value>` arg; the two bools are `--skin-random-male` / `--skin-random-female`.
    /// `baseq2` is used to enumerate/resolve skins. Returns the userinfo-ready selection or
    /// an error string (unknown skin, or more than one flag).
    pub fn from_cli(
        baseq2: &Path,
        skin: Option<&str>,
        random_male: bool,
        random_female: bool,
    ) -> Result<Self, String> {
        match (skin, random_male, random_female) {
            (None, false, false) => Ok(Self::Default),
            (Some(name), false, false) => resolve_named_skin(baseq2, name)
                .map(Self::Fixed)
                .ok_or_else(|| {
                    format!(
                        "unknown skin '{name}' — pass `model/skin` (e.g. male/grunt) or a known \
                         skin name (male: {}; female: {})",
                        MALE_SKINS.join(", "),
                        FEMALE_SKINS.join(", "),
                    )
                }),
            (None, true, false) => Ok(Self::Random(model_skin_pool(baseq2, "male", MALE_SKINS))),
            (None, false, true) => Ok(Self::Random(model_skin_pool(
                baseq2,
                "female",
                FEMALE_SKINS,
            ))),
            _ => Err("choose only one of --skin, --skin-random-male, --skin-random-female".into()),
        }
    }

    /// The `model/skin` for one bot. `Default` → `None` (keep the userinfo default);
    /// `Fixed` → the same skin for all; `Random` → a draw from the pool via `rng`.
    pub fn per_bot(&self, rng: &mut Rng) -> Option<String> {
        match self {
            Self::Default => None,
            Self::Fixed(s) => Some(s.clone()),
            Self::Random(pool) => rng.pick(pool).cloned(),
        }
    }
}

/// Resolve a `--skin` value to a `model/skin`. A value already containing `/` is taken
/// verbatim. A bare name is matched to its owning model: first by scanning
/// `baseq2/players/<model>/<name>.pcx`, then against the built-in male/female sets.
fn resolve_named_skin(baseq2: &Path, name: &str) -> Option<String> {
    if name.contains('/') {
        return Some(name.to_string());
    }
    // Loose dirs first (picks up cyborg / custom models too).
    let players = baseq2.join("players");
    if let Ok(rd) = std::fs::read_dir(&players) {
        let mut models: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        models.sort();
        for m in &models {
            if players.join(m).join(format!("{name}.pcx")).exists() {
                return Some(format!("{m}/{name}"));
            }
        }
    }
    // Built-in fallback.
    if MALE_SKINS.contains(&name) {
        return Some(format!("male/{name}"));
    }
    if FEMALE_SKINS.contains(&name) {
        return Some(format!("female/{name}"));
    }
    None
}

/// The `model/skin` pool for `model`: scanned from `baseq2/players/<model>` when present,
/// else the built-in `fallback` list. Never empty (callers can `pick` safely).
fn model_skin_pool(baseq2: &Path, model: &str, fallback: &[&str]) -> Vec<String> {
    let mut names = scan_model_skins(baseq2, model);
    if names.is_empty() {
        names = fallback.iter().map(|s| s.to_string()).collect();
    }
    names.into_iter().map(|s| format!("{model}/{s}")).collect()
}

/// Bare skin names found in `baseq2/players/<model>` — `*.pcx` minus the `_i` icons and
/// the non-skin `skin.pcx` / `weapon.pcx`. Empty if the dir is absent.
fn scan_model_skins(baseq2: &Path, model: &str) -> Vec<String> {
    let dir = baseq2.join("players").join(model);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("pcx") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem.ends_with("_i") || stem == "skin" || stem == "weapon" {
                continue;
            }
            out.push(stem.to_string());
        }
    }
    out.sort();
    out
}

/// A tiny seedable PRNG (SplitMix64) so random skins need no external crate. Seeded per
/// process from the wall clock XOR pid, so each fleet run draws a different sequence.
pub struct Rng(u64);

impl Rng {
    /// Seed from the current time and process id.
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Self(nanos ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    /// Next 64-bit value (SplitMix64).
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniformly chosen element of `items` (`None` only if empty).
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        Some(&items[(self.next_u64() % items.len() as u64) as usize])
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn no_baseq2() -> PathBuf {
        PathBuf::from("/nonexistent-baseq2")
    }

    #[test]
    fn no_flags_is_default() {
        let s = SkinSelection::from_cli(&no_baseq2(), None, false, false).unwrap();
        assert_eq!(s, SkinSelection::Default);
        assert_eq!(s.per_bot(&mut Rng::new()), None);
    }

    #[test]
    fn bare_name_resolves_to_model_via_builtin() {
        // No loose players dir → built-in male/female sets resolve the model.
        let s = SkinSelection::from_cli(&no_baseq2(), Some("sniper"), false, false).unwrap();
        assert_eq!(s, SkinSelection::Fixed("male/sniper".into()));
        let s = SkinSelection::from_cli(&no_baseq2(), Some("cobalt"), false, false).unwrap();
        assert_eq!(s, SkinSelection::Fixed("female/cobalt".into()));
    }

    #[test]
    fn slashed_value_is_verbatim() {
        let s = SkinSelection::from_cli(&no_baseq2(), Some("male/grunt"), false, false).unwrap();
        assert_eq!(s, SkinSelection::Fixed("male/grunt".into()));
    }

    #[test]
    fn unknown_skin_errors() {
        assert!(SkinSelection::from_cli(&no_baseq2(), Some("nope"), false, false).is_err());
    }

    #[test]
    fn random_pools_use_builtin_when_no_dir() {
        let male = SkinSelection::from_cli(&no_baseq2(), None, true, false).unwrap();
        match male {
            SkinSelection::Random(pool) => {
                assert_eq!(pool.len(), MALE_SKINS.len());
                assert!(pool.iter().all(|s| s.starts_with("male/")));
                assert!(pool.contains(&"male/sniper".to_string()));
            }
            other => panic!("expected Random, got {other:?}"),
        }
        let female = SkinSelection::from_cli(&no_baseq2(), None, false, true).unwrap();
        match female {
            SkinSelection::Random(pool) => {
                assert_eq!(pool.len(), FEMALE_SKINS.len());
                assert!(pool.iter().all(|s| s.starts_with("female/")));
            }
            other => panic!("expected Random, got {other:?}"),
        }
    }

    #[test]
    fn random_per_bot_draws_from_pool() {
        let sel = SkinSelection::Random(vec!["male/grunt".into(), "male/sniper".into()]);
        let mut rng = Rng::new();
        for _ in 0..20 {
            let s = sel.per_bot(&mut rng).unwrap();
            assert!(s == "male/grunt" || s == "male/sniper");
        }
    }

    #[test]
    fn conflicting_flags_error() {
        assert!(SkinSelection::from_cli(&no_baseq2(), None, true, true).is_err());
        assert!(SkinSelection::from_cli(&no_baseq2(), Some("sniper"), true, false).is_err());
    }
}
