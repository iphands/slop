#!/usr/bin/env bash
# vendor-prep.sh — fetch and extract the source RPMs for the packages THIS BOX RUNS.
#
# The point is not "get some mesa source". It is "get the exact source that produced the
# binary currently loaded into Palworld", so that reading the code tells you about the
# thing you measured. Two ways that goes wrong, both handled here:
#
#   1. dnf resolves the NEWEST AVAILABLE version, not the installed one. Verified on
#      2026-07-31: installed mesa was 26.3.0-0.3.20260801.00.6c306fd, and
#      `dnf download --srpm mesa-vulkan-drivers` handed back ...20260729.05.21dc9d4 —
#      a three-day-old snapshot, silently. We pin to %{SOURCERPM} of the installed
#      binary instead of asking for a package name.
#
#   2. Local repo metadata goes stale, so even the pinned NEVRA "does not exist".
#      Same session: the exact installed EVR resolved only after --refresh. Every
#      resolve below therefore passes --refresh. This is what makes the script correct
#      across the RPM updates this box takes.
#
# Mesa here comes from a COPR (xxmitsu/mesa-git), not Fedora, so its SRPM lives in the
# COPR results tree; the kernel comes from Fedora, whose source repos ship disabled and
# must be enabled per-invocation. Both cases are handled.
#
# Patches are NOT stored here. vendor/ is regenerable and gitignored; our patches live in
# patches/<component>/ under git and are applied at build time by vendor-build.sh.

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$REPO_DIR/vendor"
SRPM_DIR="$VENDOR/srpms"

# Fedora ships its source repos disabled. Enabling them per-invocation beats enabling
# them permanently, which would slow every unrelated dnf transaction on the box.
FEDORA_SOURCE_REPOS="fedora-source,updates-source,updates-testing-source"

components=()
skip_builddep=0

usage() {
    cat <<'EOF'
Usage: vendor-prep.sh [--component mesa|kernel] [--no-builddep]

  --component NAME   prepare just this one (repeatable). Default: mesa and kernel.
  --no-builddep      skip `dnf builddep`, which is the only step needing sudo.

For each component this:
  1. resolves the source RPM of the INSTALLED build (not the newest available),
  2. downloads it into vendor/srpms/ if not already there,
  3. extracts it into vendor/<component>/{SPECS,SOURCES},
  4. installs its build dependencies.

Re-running is cheap and is how you resync after `dnf update` changes mesa or the kernel:
the installed NEVRA changes, so a new SRPM is fetched and extracted alongside the old one.

Component -> what the installed version is read from:
  mesa    mesa-vulkan-drivers  (the package owning /usr/lib64/libvulkan_intel.so)
  kernel  kernel-core-$(uname -r)  (the RUNNING kernel, not the newest installed one)
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --component)   components+=("$2"); shift 2 ;;
        --no-builddep) skip_builddep=1; shift ;;
        -h|--help)     usage; exit 0 ;;
        *) echo "vendor-prep: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done
if [[ ${#components[@]} -eq 0 ]]; then
    components=(mesa kernel)
fi

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "vendor-prep: missing '$1' — dnf install $2" >&2
        exit 1
    }
}
need rpm rpm
need rpm2cpio rpm
need cpio cpio
need dnf dnf5

# --- which build is actually installed --------------------------------------------------
# Returns the .src.rpm filename recorded in the installed binary package's header. This is
# authoritative: it is what the build system stamped in, not what a repo currently offers.
installed_srpm_for() {
    local component=$1 pkg
    case "$component" in
        mesa)   pkg="mesa-vulkan-drivers" ;;
        kernel) pkg="kernel-core-$(uname -r)" ;;
        *) echo "vendor-prep: unknown component '$component'" >&2; return 1 ;;
    esac
    # A multilib box has both i686 and x86_64 mesa installed and they can differ; take the
    # x86_64 one, since that is the driver the 64-bit game loads.
    rpm -q --qf '%{ARCH} %{SOURCERPM}\n' "$pkg" 2>/dev/null \
        | awk '$1 == "x86_64" || $1 == "noarch" { print $2; exit }'
}

# NEVRA that `dnf download` understands, derived from the srpm filename.
nevra_from_srpm() { printf '%s\n' "${1%.src.rpm}"; }

repo_flags_for() {
    case "$1" in
        mesa)   printf '%s\n' "--refresh" ;;
        kernel) printf '%s\n' "--refresh --enablerepo=$FEDORA_SOURCE_REPOS" ;;
    esac
}

# --- download ---------------------------------------------------------------------------
fetch_srpm() {
    local component=$1 srpm=$2 nevra flags
    nevra=$(nevra_from_srpm "$srpm")
    read -r -a flags <<<"$(repo_flags_for "$component")"

    if [[ -f "$SRPM_DIR/$srpm" ]]; then
        echo "vendor-prep: [$component] already have $srpm"
        return 0
    fi

    echo "vendor-prep: [$component] resolving $nevra ..."
    # Judged on the artefact, not on dnf's exit status: a pipeline's status is `tail`'s,
    # and dnf can also exit 0 having downloaded something other than what we pinned.
    dnf download --srpm "${flags[@]}" --destdir "$SRPM_DIR" "$nevra" 2>&1 | tail -3 || true
    if [[ -f "$SRPM_DIR/$srpm" ]]; then
        return 0
    fi

    # Fedora prunes older builds out of updates-source; koji keeps everything. This is the
    # normal path for a kernel that is a few weeks old, not an error case.
    echo "vendor-prep: [$component] not in a repo — trying koji" >&2
    need koji koji
    ( cd "$SRPM_DIR" && koji download-build --arch=src "$nevra" ) || {
        echo "vendor-prep: [$component] FAILED to obtain $srpm" >&2
        echo "vendor-prep: the build is in neither the repos nor koji. If this box is" >&2
        echo "vendor-prep: running a COPR build that has since been pruned, the source" >&2
        echo "vendor-prep: for the binary you are measuring is simply gone — update the" >&2
        echo "vendor-prep: package and re-run rather than studying a different build." >&2
        return 1
    }
    [[ -f "$SRPM_DIR/$srpm" ]]
}

# --- extract ----------------------------------------------------------------------------
# `rpm -i` on a source RPM with a private _topdir lays out SPECS/ and SOURCES/ exactly the
# way rpmbuild expects, which is why this is not a rpm2cpio-into-a-directory.
extract_srpm() {
    local component=$1 srpm=$2 topdir="$VENDOR/$component"
    local stamp="$topdir/.extracted-from"

    if [[ -f $stamp && "$(cat "$stamp")" == "$srpm" ]]; then
        echo "vendor-prep: [$component] already extracted $srpm"
        return 0
    fi

    echo "vendor-prep: [$component] extracting $srpm -> $topdir"
    mkdir -p "$topdir"/{SPECS,SOURCES,BUILD,BUILDROOT,RPMS,SRPMS}
    rpm --define "_topdir $topdir" -i "$SRPM_DIR/$srpm"
    printf '%s\n' "$srpm" >"$stamp"
}

# --- build deps -------------------------------------------------------------------------
install_builddeps() {
    local component=$1 spec
    spec=$(find "$VENDOR/$component/SPECS" -maxdepth 1 -name '*.spec' | head -1)
    if [[ -z $spec ]]; then
        echo "vendor-prep: [$component] no spec found — cannot resolve build deps" >&2
        return 1
    fi
    echo "vendor-prep: [$component] installing build deps from ${spec##*/} (needs sudo)"
    sudo dnf builddep -y "$spec"
}

# --- main -------------------------------------------------------------------------------
mkdir -p "$SRPM_DIR"
failed=()

for component in "${components[@]}"; do
    echo
    echo "=== $component ============================================================"
    srpm=$(installed_srpm_for "$component") || { failed+=("$component"); continue; }
    if [[ -z $srpm ]]; then
        echo "vendor-prep: [$component] could not determine the installed build" >&2
        failed+=("$component"); continue
    fi
    echo "vendor-prep: [$component] installed build -> $srpm"

    fetch_srpm   "$component" "$srpm" || { failed+=("$component"); continue; }
    extract_srpm "$component" "$srpm" || { failed+=("$component"); continue; }
    if [[ $skip_builddep -eq 0 ]]; then
        install_builddeps "$component" || { failed+=("$component"); continue; }
    fi

    spec=$(find "$VENDOR/$component/SPECS" -maxdepth 1 -name '*.spec' | head -1)
    echo "vendor-prep: [$component] READY"
    echo "vendor-prep: [$component]   spec    ${spec:-<none>}"
    echo "vendor-prep: [$component]   sources $(find "$VENDOR/$component/SOURCES" -type f | wc -l) files"
done

echo
if [[ ${#failed[@]} -gt 0 ]]; then
    echo "vendor-prep: FAILED for: ${failed[*]}" >&2
    exit 1
fi
echo "vendor-prep: all components ready under $VENDOR"
echo "vendor-prep: next -> ./scripts/vendor-build.sh --component mesa"
