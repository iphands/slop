//! Extracting a human-readable package identity from a path.
//!
//! Top-N lists keyed on the raw path are unusable — real production paths run
//! 60–110 characters, e.g.
//! `/debian/pool/main/libx/libxml2/libxml2-utils_2.12.7+dfsg+really2.9.14-2.1+deb13u3_amd64.deb`.
//! The dashboard shows the parsed name with the version muted beside it, and
//! keeps the full path in a tooltip.
//!
//! Both parsers are validated against filenames taken from real traffic
//! (2026-07-18). The inputs are expected to be **already percent-decoded** by
//! [`crate::classify`].

/// A parsed package filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgName<'a> {
    pub name: &'a str,
    pub version: &'a str,
    /// rpm only; empty for deb, whose release is part of the version.
    pub release: &'a str,
    pub arch: &'a str,
}

/// Parse a Debian package filename: `name_version_arch.deb`.
///
/// Splitting on `_` is safe precisely because a Debian version may contain `-`
/// and `+` freely but **never** `_`.
///
/// ```text
/// libxml2-utils_2.12.7+dfsg+really2.9.14-2.1+deb13u3_amd64.deb
///   -> name="libxml2-utils"
///      version="2.12.7+dfsg+really2.9.14-2.1+deb13u3"
///      arch="amd64"
/// ```
pub fn parse_deb(filename: &str) -> Option<PkgName<'_>> {
    let stem = filename
        .strip_suffix(".deb")
        .or_else(|| filename.strip_suffix(".udeb"))?;
    let mut parts = stem.split('_');
    let name = parts.next()?;
    let version = parts.next()?;
    let arch = parts.next()?;
    if parts.next().is_some() || name.is_empty() || version.is_empty() || arch.is_empty() {
        return None;
    }
    Some(PkgName {
        name,
        version,
        release: "",
        arch,
    })
}

/// Parse an RPM filename: `name-version-release.arch.rpm`.
///
/// Split from the **right**: `.rpm`, then `.arch`, then `-release`, then
/// `-version`; whatever remains is the name. Splitting from the left is wrong
/// because rpm names contain `-` — see
/// `pipewire-jack-audio-connection-kit-libs-1.6.8-1.fc44.x86_64.rpm`.
pub fn parse_rpm(filename: &str) -> Option<PkgName<'_>> {
    let stem = filename.strip_suffix(".rpm")?;
    let (rest, arch) = stem.rsplit_once('.')?;
    let (rest, release) = rest.rsplit_once('-')?;
    let (name, version) = rest.rsplit_once('-')?;
    if name.is_empty() || version.is_empty() || release.is_empty() || arch.is_empty() {
        return None;
    }
    Some(PkgName {
        name,
        version,
        release,
        arch,
    })
}

/// Parse whichever format the path's extension indicates.
///
/// Returns `None` for metadata and for anything unparseable — the caller must
/// then fall back to showing the full path, never drop the row.
pub fn parse_path(path: &str) -> Option<PkgName<'_>> {
    let filename = path.rsplit('/').next()?;
    if filename.ends_with(".rpm") {
        parse_rpm(filename)
    } else {
        parse_deb(filename)
    }
}

/// Is this segment a bare content hash? apt's `by-hash/` indices are named
/// after their own SHA, which is meaningless as a table label.
fn looks_like_hash(s: &str) -> bool {
    s.len() >= 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// A display label for a path: the package name if parseable, else something a
/// human can actually read. Never empty, never drops information silently.
///
/// The `by-hash` case matters in practice — apt fetches most of its indices that
/// way, so a naive "last path segment" turns the whole top-metadata list into a
/// column of indistinguishable 64-character hashes. That is exactly what the
/// first rendered screenshot of the dashboard showed. Fall back to the
/// enclosing directory, which names the actual index.
pub fn display_name(path: &str) -> &str {
    if let Some(p) = parse_path(path) {
        return p.name;
    }
    let mut segs = path.rsplit('/').filter(|s| !s.is_empty());
    let Some(last) = segs.next() else { return path };
    if looks_like_hash(last) {
        // .../binary-amd64/by-hash/SHA256/<hash>  ->  "binary-amd64"
        for seg in segs {
            if seg != "by-hash" && !seg.eq_ignore_ascii_case("SHA256") && !looks_like_hash(seg) {
                return seg;
            }
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every case below is a filename observed in the operator's production logs
    // on 2026-07-18, decoded. They are the regression suite for this module.

    #[test]
    fn a_simple_deb_parses() {
        let p = parse_deb("libgl1-mesa-dri_25.0.7-2+deb13u1_amd64.deb").unwrap();
        assert_eq!(p.name, "libgl1-mesa-dri");
        assert_eq!(p.version, "25.0.7-2+deb13u1");
        assert_eq!(p.arch, "amd64");
    }

    #[test]
    fn a_deb_version_full_of_plusses_and_dashes_parses() {
        // The worst real example: '+dfsg+really' and an embedded '-'.
        let p = parse_deb("libxml2-utils_2.12.7+dfsg+really2.9.14-2.1+deb13u3_amd64.deb").unwrap();
        assert_eq!(p.name, "libxml2-utils");
        assert_eq!(p.version, "2.12.7+dfsg+really2.9.14-2.1+deb13u3");
        assert_eq!(p.arch, "amd64");
    }

    #[test]
    fn a_deb_with_ds_in_the_version_parses() {
        let p = parse_deb("librbd1_18.2.7+ds-1+deb13u1_amd64.deb").unwrap();
        assert_eq!(p.name, "librbd1");
        assert_eq!(p.version, "18.2.7+ds-1+deb13u1");
    }

    #[test]
    fn a_deb_with_arch_all_parses() {
        let p = parse_deb("python3-idna_3.10-1+deb13u1_all.deb").unwrap();
        assert_eq!(p.name, "python3-idna");
        assert_eq!(p.arch, "all");
    }

    #[test]
    fn a_udeb_parses() {
        let p = parse_deb("foo-udeb_1.0_amd64.udeb").unwrap();
        assert_eq!(p.name, "foo-udeb");
    }

    #[test]
    fn a_simple_rpm_parses() {
        let p = parse_rpm("glib2-2.88.2-1.fc44.x86_64.rpm").unwrap();
        assert_eq!(p.name, "glib2");
        assert_eq!(p.version, "2.88.2");
        assert_eq!(p.release, "1.fc44");
        assert_eq!(p.arch, "x86_64");
    }

    #[test]
    fn an_rpm_whose_name_contains_dashes_parses() {
        // This is why rpm must be split from the RIGHT.
        let p =
            parse_rpm("pipewire-jack-audio-connection-kit-libs-1.6.8-1.fc44.x86_64.rpm").unwrap();
        assert_eq!(p.name, "pipewire-jack-audio-connection-kit-libs");
        assert_eq!(p.version, "1.6.8");
        assert_eq!(p.release, "1.fc44");
        assert_eq!(p.arch, "x86_64");
    }

    #[test]
    fn an_rpm_with_a_caret_in_the_version_parses() {
        // usbmuxd's version arrives percent-encoded as %5e and decodes to '^'.
        let p = parse_rpm("usbmuxd-1.1.1^20251205git3ded00c-1.fc44.x86_64.rpm").unwrap();
        assert_eq!(p.name, "usbmuxd");
        assert_eq!(p.version, "1.1.1^20251205git3ded00c");
    }

    #[test]
    fn a_noarch_rpm_with_a_long_dashed_name_parses() {
        let p = parse_rpm("julietaula-montserrat-fonts-9.000-4.fc44.noarch.rpm").unwrap();
        assert_eq!(p.name, "julietaula-montserrat-fonts");
        assert_eq!(p.version, "9.000");
        assert_eq!(p.arch, "noarch");
    }

    #[test]
    fn a_python_rpm_parses() {
        let p = parse_rpm("python3-pycparser-2.22-8.fc44.noarch.rpm").unwrap();
        assert_eq!(p.name, "python3-pycparser");
    }

    #[test]
    fn parse_path_dispatches_on_extension() {
        assert_eq!(
            parse_path("/debian/pool/main/m/mesa/libgbm1_25.0.7-2+deb13u1_amd64.deb")
                .unwrap()
                .name,
            "libgbm1"
        );
        assert_eq!(
            parse_path("/fedora/linux/updates/44/x/glib2-2.88.2-1.fc44.x86_64.rpm")
                .unwrap()
                .name,
            "glib2"
        );
    }

    #[test]
    fn an_unparseable_filename_falls_back_rather_than_being_dropped() {
        assert!(parse_path("/debian/dists/trixie/InRelease").is_none());
        // ...and display_name still returns something useful.
        assert_eq!(display_name("/debian/dists/trixie/InRelease"), "InRelease");
        assert_eq!(display_name("/"), "/");
    }

    #[test]
    fn a_by_hash_index_shows_its_directory_not_a_bare_hash() {
        // REGRESSION: the first screenshot's "top metadata" list was a column of
        // indistinguishable 64-char hashes.
        assert_eq!(
            display_name(
                "/debian/dists/trixie/main/binary-amd64/by-hash/SHA256/\
                 e32a0c328ac8716e71e3f66e87366a172fea8ecb2f452909abcdef0123456789"
            ),
            "binary-amd64"
        );
        // A normal metadata file is unaffected.
        assert_eq!(display_name("/debian/dists/trixie/InRelease"), "InRelease");
    }

    #[test]
    fn a_malformed_deb_with_too_many_underscores_is_rejected() {
        assert!(parse_deb("a_b_c_d.deb").is_none());
    }

    #[test]
    fn a_malformed_rpm_without_enough_dashes_is_rejected() {
        assert!(parse_rpm("noversion.rpm").is_none());
        assert!(parse_rpm("only-one.x86_64.rpm").is_none());
    }
}
