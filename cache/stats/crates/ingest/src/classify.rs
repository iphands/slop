//! Turning a `$request_uri` into `(repo, kind, path)`.
//!
//! # Why the input must be `$request_uri` and never `$uri`
//!
//! `proxy/conf.d/pkgcache.conf` rewrites `/fedora/…` → `/pub/fedora/…` inside
//! the `.rpm` sub-location, so `$uri` for every Fedora *package* begins
//! `/pub/fedora/`. Metadata is served by the parent location, which does no
//! rewrite, so it would classify correctly — meaning a dashboard fed from `$uri`
//! looks about 90% right while filing every `.rpm` under a repo named `pub`.
//!
//! Measured live 2026-07-18; see `context/pitfalls.md`. The test
//! `a_pub_fedora_path_is_the_wrong_shape_and_means_uri_was_logged` exists so
//! that anyone who "simplifies" nginx back to `$uri` gets a red test rather
//! than a quietly wrong dashboard.

/// What sort of file a request was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An immutable package file: `.deb`, `.udeb`, `.rpm`.
    Package,
    /// Repo metadata — indices, `repomd.xml`, `by-hash/…`. Short TTL, and
    /// *supposed* to miss often, which is why it is never blended with packages.
    Metadata,
    /// Neither: `/`, `/healthz`, a malformed URI.
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Metadata => "metadata",
            Self::Other => "other",
        }
    }
}

/// Repo shown for a URI we could not attribute to one.
pub const REPO_UNKNOWN: &str = "-";

/// Longest path we store. Real paths run 60–110 chars; the cap bounds a
/// pathological or hostile URI without truncating anything genuine.
pub const MAX_PATH_LEN: usize = 512;

/// A classified request path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    /// First path segment: `debian`, `debian-security`, `fedora`, or `-`.
    pub repo: String,
    pub kind: Kind,
    /// Percent-decoded, query-stripped, `//`-collapsed, length-capped path.
    pub path: String,
    /// True when percent-decoding produced invalid UTF-8 and the raw form was
    /// kept. Surfaced as a parse anomaly rather than dropping the line.
    pub decode_failed: bool,
}

/// Classify a `$request_uri`.
///
/// The path is **percent-decoded** before it is stored. Production logs show
/// encoding is pervasive, not exotic — 42 occurrences of `%2b` (`+`) in a single
/// 32-package apt run, because Debian versions are full of `+`
/// (`18.2.7+ds-1+deb13u1`); Fedora uses `%5e` (`^`). Storing the raw form would
/// make paths unreadable in the UI *and* let a client that encodes differently
/// create a second, unrelated row for the same package.
pub fn classify(uri: &str) -> Classified {
    // Strip the query string: a cache-busting parameter would otherwise turn one
    // package into thousands of rows.
    let no_query = uri.split(['?', '#']).next().unwrap_or("");

    let (decoded, decode_failed) = match percent_decode(no_query) {
        Some(d) => (d, false),
        None => (no_query.to_string(), true),
    };

    let collapsed = collapse_slashes(&decoded);
    let path = truncate(collapsed, MAX_PATH_LEN);

    let repo = first_segment(&path).unwrap_or(REPO_UNKNOWN).to_string();
    let kind = if repo == REPO_UNKNOWN {
        Kind::Other
    } else if is_package(&path) {
        Kind::Package
    } else {
        Kind::Metadata
    };

    Classified {
        repo,
        kind,
        path,
        decode_failed,
    }
}

/// The first non-empty path segment, or `None` for a URI with no leading `/` or
/// an empty first segment.
fn first_segment(path: &str) -> Option<&str> {
    let rest = path.strip_prefix('/')?;
    let seg = rest.split('/').next()?;
    if seg.is_empty() {
        None
    } else {
        Some(seg)
    }
}

fn is_package(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".deb") || lower.ends_with(".udeb") || lower.ends_with(".rpm")
}

/// Decode `%XX` escapes. Returns `None` if the result is not valid UTF-8, so the
/// caller can keep the raw form rather than dropping the line.
fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_string());
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        // A stray '%' that is not a valid escape falls through and is kept
        // verbatim, rather than dropping the line.
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn collapse_slashes(s: &str) -> String {
    if !s.contains("//") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    out
}

fn truncate(mut s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    // Cut on a char boundary so the result stays valid UTF-8.
    let mut cut = max.saturating_sub(1);
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fedora_rpm_is_a_package_in_the_fedora_repo() {
        let c = classify(
            "/fedora/linux/updates/44/Everything/x86_64/Packages/g/glib2-2.88.2-1.fc44.x86_64.rpm",
        );
        assert_eq!(c.repo, "fedora");
        assert_eq!(c.kind, Kind::Package);
    }

    /// REGRESSION GUARD. `/pub/fedora/...` is what `$uri` yields after the
    /// `.rpm` sub-location's rewrite. If nginx is ever "simplified" back to
    /// logging `$uri`, every Fedora package lands in a repo called `pub` and
    /// this test is what catches it. See context/pitfalls.md.
    #[test]
    fn a_pub_fedora_path_is_the_wrong_shape_and_means_uri_was_logged() {
        let c = classify("/pub/fedora/linux/updates/44/Everything/x86_64/Packages/g/glib2.rpm");
        assert_eq!(
            c.repo, "pub",
            "if this ever appears in real data, nginx is logging $uri, not $request_uri"
        );
        assert_ne!(c.repo, "fedora");
    }

    #[test]
    fn debian_security_is_its_own_repo_with_no_special_case() {
        let c =
            classify("/debian-security/pool/updates/main/l/linux/linux-libc-dev_6.12.95-1_all.deb");
        assert_eq!(c.repo, "debian-security");
        assert_eq!(c.kind, Kind::Package);
    }

    #[test]
    fn metadata_and_packages_are_distinguished_by_extension() {
        assert_eq!(
            classify("/debian/dists/trixie/InRelease").kind,
            Kind::Metadata
        );
        assert_eq!(
            classify("/fedora/linux/releases/44/Everything/x86_64/os/repodata/repomd.xml").kind,
            Kind::Metadata
        );
        assert_eq!(
            classify("/debian/pool/main/c/cowsay/cowsay_3.03_all.deb").kind,
            Kind::Package
        );
        assert_eq!(
            classify("/debian/pool/main/x/x/foo_1_all.udeb").kind,
            Kind::Package
        );
    }

    #[test]
    fn a_by_hash_index_is_metadata() {
        let c = classify("/debian/dists/trixie/main/binary-amd64/by-hash/SHA256/abc123");
        assert_eq!(c.kind, Kind::Metadata);
        assert_eq!(c.repo, "debian");
    }

    #[test]
    fn the_banner_and_a_bare_slash_are_other() {
        assert_eq!(classify("/").kind, Kind::Other);
        assert_eq!(classify("/").repo, REPO_UNKNOWN);
        assert_eq!(classify("").kind, Kind::Other);
        assert_eq!(classify("no-leading-slash").kind, Kind::Other);
    }

    #[test]
    fn percent_encoding_is_decoded() {
        // %2b -> '+' is pervasive in Debian versions; %5e -> '^' in Fedora.
        let c = classify("/debian/pool/main/m/mesa/libgl1-mesa-dri_25.0.7-2%2bdeb13u1_amd64.deb");
        assert!(c
            .path
            .ends_with("libgl1-mesa-dri_25.0.7-2+deb13u1_amd64.deb"));
        assert!(!c.decode_failed);

        let c = classify("/fedora/x/usbmuxd-1.1.1%5e20251205git3ded00c-1.fc44.x86_64.rpm");
        assert!(c.path.contains("1.1.1^20251205git3ded00c"));
    }

    #[test]
    fn the_raw_and_decoded_forms_of_one_package_collapse_to_one_row() {
        // The whole point of decoding: two clients encoding differently must not
        // produce two unrelated rows for the same file.
        let encoded =
            classify("/debian/pool/main/c/ceph/librbd1_18.2.7%2bds-1%2bdeb13u1_amd64.deb");
        let plain = classify("/debian/pool/main/c/ceph/librbd1_18.2.7+ds-1+deb13u1_amd64.deb");
        assert_eq!(encoded.path, plain.path);

        // ...including when the hex case differs (RFC 3986 says they are equal).
        let upper = classify("/debian/pool/main/c/ceph/librbd1_18.2.7%2Bds-1%2Bdeb13u1_amd64.deb");
        assert_eq!(upper.path, plain.path);
    }

    #[test]
    fn a_query_string_is_stripped() {
        let c = classify("/debian/pool/main/c/cowsay/cowsay_3.03_all.deb?cachebust=12345");
        assert!(c.path.ends_with(".deb"));
        assert!(!c.path.contains('?'));
        assert_eq!(c.kind, Kind::Package, "the extension check runs post-strip");
    }

    #[test]
    fn duplicate_slashes_are_collapsed() {
        assert_eq!(
            classify("/debian//dists///trixie/InRelease").path,
            "/debian/dists/trixie/InRelease"
        );
    }

    #[test]
    fn an_overlong_path_is_capped_and_marked() {
        let long = format!("/debian/{}", "a".repeat(MAX_PATH_LEN * 2));
        let c = classify(&long);
        assert!(c.path.chars().count() <= MAX_PATH_LEN);
        assert!(c.path.ends_with('…'));
    }

    #[test]
    fn a_stray_percent_is_kept_rather_than_dropping_the_line() {
        let c = classify("/debian/100%-sure");
        assert!(c.path.contains('%'));
        assert!(!c.decode_failed);
    }

    #[test]
    fn an_uppercase_extension_still_counts_as_a_package() {
        assert_eq!(classify("/debian/pool/x/FOO_1_ALL.DEB").kind, Kind::Package);
    }
}
