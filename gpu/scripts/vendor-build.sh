#!/usr/bin/env bash
# vendor-build.sh — rebuild a vendored SRPM with OUR patches applied, producing RPMs.
#
# Patches live in patches/<component>/ and are under git. vendor/ is regenerable and
# gitignored, so nothing we author may live there — this script copies the pristine spec,
# injects our patches into the copy, and builds that. The pristine tree stays pristine, and
# `git diff` always shows the whole of our work.
#
# Patch application is spec-shape dependent, and getting it wrong silently produces an
# UNPATCHED build that then "measures" as having no effect — the same failure class as a
# local driver that never loaded (context/pitfalls.md). So each supported shape is detected
# explicitly and an unrecognised spec is a hard error, never a best-effort build:
#
#   %autosetup      patches listed as PatchNNNN: are applied automatically by %autopatch.
#   %setup          explicit `%patch -PNNNN -p1` lines are appended to %prep.
#   Fedora kernel   has its own machinery; our patches are concatenated into the
#                   linux-kernel-test.patch slot the spec already applies.

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$REPO_DIR/vendor"
PATCH_ROOT="$REPO_DIR/patches"

# High enough not to collide with the Patch numbers a distro spec already uses.
PATCH_BASE=9000
KERNEL_TEST_PATCH="linux-kernel-test.patch"

component=""
apply_patches=1
rpmbuild_extra=()

usage() {
    cat <<'EOF'
Usage: vendor-build.sh --component NAME [--no-patches] [-- <extra rpmbuild args>]

  --component NAME  mesa | kernel (must have been prepared by vendor-prep.sh)
  --no-patches      build the PRISTINE distro source. This is not a debug option — it is
                    how you produce the baseline that a patched build is compared against,
                    built by the same toolchain on the same box.
  --                everything after is passed to rpmbuild verbatim.

Patches are read from patches/<component>/, in the order given by patches/<component>/series
if that file exists, else sorted by filename. An empty or absent directory is fine and
builds pristine.

Kernel builds are large. The defaults below drop the variants we never boot; override by
passing your own --without/--with after `--`.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --component)  component="$2"; shift 2 ;;
        --no-patches) apply_patches=0; shift ;;
        --)           shift; rpmbuild_extra=("$@"); break ;;
        -h|--help)    usage; exit 0 ;;
        *) echo "vendor-build: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ -z $component ]]; then
    echo "vendor-build: --component is required" >&2; usage >&2; exit 2
fi

TOPDIR="$VENDOR/$component"
if [[ ! -d "$TOPDIR/SPECS" ]]; then
    echo "vendor-build: $component not prepared — run ./scripts/vendor-prep.sh first" >&2
    exit 1
fi

pristine_spec=$(find "$TOPDIR/SPECS" -maxdepth 1 -name '*.spec' ! -name '*.patched.spec' | head -1)
if [[ -z $pristine_spec ]]; then
    echo "vendor-build: no spec in $TOPDIR/SPECS" >&2; exit 1
fi

# --- collect our patches ----------------------------------------------------------------
patches=()
patch_dir="$PATCH_ROOT/$component"
if [[ $apply_patches -eq 1 && -d $patch_dir ]]; then
    if [[ -f "$patch_dir/series" ]]; then
        # A series file makes patch ORDER explicit and reviewable. Without it, ordering is
        # whatever the filenames sort to, which is fine until two patches touch one file.
        while read -r name; do
            [[ -z $name || $name == \#* ]] && continue
            if [[ ! -f "$patch_dir/$name" ]]; then
                echo "vendor-build: series lists '$name' but $patch_dir/$name is missing" >&2
                exit 1
            fi
            patches+=("$patch_dir/$name")
        done <"$patch_dir/series"
    else
        while IFS= read -r p; do patches+=("$p"); done \
            < <(find "$patch_dir" -maxdepth 1 -name '*.patch' | sort)
    fi
fi

echo "vendor-build: component  $component"
echo "vendor-build: spec       ${pristine_spec##*/}"
if [[ ${#patches[@]} -eq 0 ]]; then
    echo "vendor-build: patches    NONE — this is a pristine baseline build"
else
    echo "vendor-build: patches    ${#patches[@]} from patches/$component/"
    for p in "${patches[@]}"; do echo "vendor-build:              ${p##*/}"; done
fi

work_spec="$TOPDIR/SPECS/$(basename "${pristine_spec%.spec}").patched.spec"
cp -- "$pristine_spec" "$work_spec"

# --- inject -----------------------------------------------------------------------------
inject_via_patch_directives() {
    local spec=$1 uses_autosetup=$2 n=$PATCH_BASE decl="" apply="" p
    for p in "${patches[@]}"; do
        cp -- "$p" "$TOPDIR/SOURCES/"
        decl+="Patch${n}: $(basename "$p")"$'\n'
        apply+="%patch -P${n} -p1"$'\n'
        n=$((n + 1))
    done

    # Declarations go immediately before %description, which every spec has and which
    # always follows the preamble. Anchoring to %description rather than "the last Patch:
    # line" means this works on specs that carry no patches at all.
    python3 - "$spec" "$decl" <<'PY'
import sys
spec, decl = sys.argv[1], sys.argv[2]
text = open(spec, encoding='utf-8').read()
idx = text.find('\n%description')
if idx < 0:
    sys.exit("vendor-build: no %description in spec; cannot place Patch declarations")
open(spec, 'w', encoding='utf-8').write(text[:idx + 1] + decl + text[idx + 1:])
PY

    if [[ $uses_autosetup -eq 1 ]]; then
        echo "vendor-build: spec uses %autosetup — %autopatch will apply them"
        return 0
    fi

    # %setup does not apply patches, so append explicit directives at the end of %prep,
    # i.e. immediately before the next top-level section.
    python3 - "$spec" "$apply" <<'PY'
import re, sys
spec, apply = sys.argv[1], sys.argv[2]
text = open(spec, encoding='utf-8').read()
m = re.search(r'^%prep\b', text, re.M)
if not m:
    sys.exit("vendor-build: no %prep section; cannot place %patch directives")
nxt = re.search(r'^%(build|conf|generate_buildrequires|install|check|files)\b',
                text[m.end():], re.M)
at = m.end() + (nxt.start() if nxt else len(text) - m.end())
open(spec, 'w', encoding='utf-8').write(text[:at] + apply + text[at:])
PY
    echo "vendor-build: spec uses %setup — appended %patch directives to %prep"
}

inject_via_kernel_test_patch() {
    local target="$TOPDIR/SOURCES/$KERNEL_TEST_PATCH" p
    if ! grep -q "$KERNEL_TEST_PATCH" "$work_spec"; then
        echo "vendor-build: kernel spec does not reference $KERNEL_TEST_PATCH." >&2
        echo "vendor-build: refusing to guess — inspect %prep in ${pristine_spec##*/} and" >&2
        echo "vendor-build: teach this script the shape rather than shipping an" >&2
        echo "vendor-build: unpatched kernel that measures as a no-op." >&2
        exit 1
    fi
    : >"$target"
    for p in "${patches[@]}"; do
        cat -- "$p" >>"$target"
        printf '\n' >>"$target"
    done
    echo "vendor-build: concatenated ${#patches[@]} patch(es) into $KERNEL_TEST_PATCH"
    echo "vendor-build: (the kernel spec applies that slot itself; no spec edit needed)"
}

if [[ ${#patches[@]} -gt 0 ]]; then
    if [[ $component == kernel ]] && grep -q "$KERNEL_TEST_PATCH" "$work_spec"; then
        inject_via_kernel_test_patch
    elif grep -qE '^\s*%autosetup\b' "$work_spec"; then
        inject_via_patch_directives "$work_spec" 1
    elif grep -qE '^\s*%setup\b' "$work_spec"; then
        inject_via_patch_directives "$work_spec" 0
    else
        echo "vendor-build: ${pristine_spec##*/} uses neither %autosetup nor %setup and is" >&2
        echo "vendor-build: not the Fedora kernel. Refusing to build, because injecting" >&2
        echo "vendor-build: patches blindly risks producing an unpatched RPM that looks" >&2
        echo "vendor-build: like a patch with no effect. Inspect %prep and extend this" >&2
        echo "vendor-build: script." >&2
        exit 1
    fi
fi

# --- build ------------------------------------------------------------------------------
opts=(--define "_topdir $TOPDIR")
if [[ $component == kernel && ${#rpmbuild_extra[@]} -eq 0 ]]; then
    # We boot exactly one kernel flavour and never use the debug variants; building them
    # roughly doubles an already long build for artefacts that go straight to the bin.
    opts+=(--without debug --without debuginfo --without doc
           --without perf --without tools --without bpftool --with baseonly)
    echo "vendor-build: kernel defaults: ${opts[*]:2}"
fi
opts+=("${rpmbuild_extra[@]}")

echo
echo "vendor-build: rpmbuild -ba ${work_spec##*/}"
echo "vendor-build: (log is long; a kernel takes tens of minutes)"
echo
rpmbuild -ba "${opts[@]}" "$work_spec"

echo
echo "vendor-build: built RPMs:"
find "$TOPDIR/RPMS" -name '*.rpm' -newer "$work_spec" -printf '  %p\n' | sort
echo
echo "vendor-build: next -> ./scripts/vendor-install.sh --component $component"
