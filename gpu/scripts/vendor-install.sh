#!/usr/bin/env bash
# vendor-install.sh — install RPMs built by vendor-build.sh, with a recorded way back.
#
# This deliberately installs system-wide rather than side-loading via VK_DRIVER_FILES.
# The reason is that the kernel half of this project cannot be side-loaded at all, and
# running mesa one way and the kernel another makes the two halves incomparable.
#
# The cost is that the box's reference driver — the thing every measurement is compared
# against — is now something we built. So the revert path is not an afterthought here: the
# exact NEVRAs being replaced are written to vendor/installed-<component>.log BEFORE the
# transaction, and the dnf history id is printed after it.
#
# Kernels are parallel-installable: installing ours leaves the distro kernel bootable, and
# the machine still boots to a known-good state if our patch panics. Mesa is not — it
# replaces. Read the revert instructions this prints before rebooting into anything.

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$REPO_DIR/vendor"

component=""
assume_yes=0

usage() {
    cat <<'EOF'
Usage: vendor-install.sh --component NAME [--yes]

  --component NAME  mesa | kernel
  --yes             do not prompt before the dnf transaction

Installs the RPMs most recently built into vendor/<component>/RPMS. Only -debuginfo and
-debugsource are skipped. -devel IS installed: the local build carries a different release
than the distro package, and devel subpackages require an exact version-release match, so
leaving them behind would break dependency resolution.

After installing mesa, VERIFY the build actually loaded before trusting any measurement:

    vulkaninfo | grep -i driverInfo

It must report the build you just installed. A driver that never loaded measures as a
perfect no-op, which is indistinguishable from a patch that does nothing.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --component) component="$2"; shift 2 ;;
        --yes)       assume_yes=1; shift ;;
        -h|--help)   usage; exit 0 ;;
        *) echo "vendor-install: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ -z $component ]]; then
    echo "vendor-install: --component is required" >&2; usage >&2; exit 2
fi

RPM_DIR="$VENDOR/$component/RPMS"
if [[ ! -d $RPM_DIR ]]; then
    echo "vendor-install: nothing built for $component — run vendor-build.sh first" >&2
    exit 1
fi

# Skip what we would only be paying disk for. -debuginfo/-debugsource in particular are
# frequently larger than everything else combined.
mapfile -t rpms < <(
    find "$RPM_DIR" -name '*.rpm' \
        ! -name '*-debuginfo-*' ! -name '*-debugsource-*' \
        | sort
)

if [[ ${#rpms[@]} -eq 0 ]]; then
    echo "vendor-install: no installable RPMs under $RPM_DIR" >&2
    exit 1
fi

echo "vendor-install: will install ${#rpms[@]} package(s):"
for r in "${rpms[@]}"; do echo "  ${r##*/}"; done

# --- record the way back BEFORE changing anything ---------------------------------------
log="$VENDOR/installed-${component}.log"
{
    echo "# vendor-install.sh $(date -Is) — component=$component"
    echo "# NEVRAs installed immediately BEFORE this transaction:"
    case "$component" in
        mesa)   rpm -qa 'mesa*' --qf '%{NEVRA}\n' | sort ;;
        kernel) rpm -qa 'kernel*' --qf '%{NEVRA}\n' | sort ;;
    esac
    echo "# RPMs being installed:"
    printf '%s\n' "${rpms[@]##*/}"
} >"$log"
echo
echo "vendor-install: pre-transaction state recorded in $log"

if [[ $component == mesa ]]; then
    cat <<'EOF'

  ┌─ THIS REPLACES THE SYSTEM MESA ─────────────────────────────────────────────┐
  Every measurement this project has taken is against the distro build. After
  this, the box's baseline is a driver we compiled. That is the intent, but it
  means "known good" is now something you restore rather than something you have.

  Revert with the history id printed at the end, or:
      sudo dnf distro-sync 'mesa*'
  └─────────────────────────────────────────────────────────────────────────────┘

EOF
fi

if [[ $assume_yes -eq 0 ]]; then
    read -r -p "Proceed? [y/N] " ans
    [[ ${ans,,} == y ]] || { echo "vendor-install: aborted"; exit 1; }
fi

sudo dnf install -y "${rpms[@]}"

echo
echo "vendor-install: done. Revert this exact transaction with:"
# `history list` is readable unprompted; running it under sudo would ask for a password
# for no reason right after the transaction already had one.
echo "    sudo dnf history undo $(dnf history list --reverse 2>/dev/null | tail -1 | awk '{print $1}')"
echo

case "$component" in
    mesa)
        echo "vendor-install: VERIFY the build loaded — this is not optional:"
        echo "    vulkaninfo | grep -i driverInfo"
        echo "vendor-install: it must name the build you just installed, not the distro one."
        ;;
    kernel)
        echo "vendor-install: reboot and select the new kernel. The distro kernel remains"
        echo "vendor-install: installed and bootable; if ours does not come up, pick the old"
        echo "vendor-install: entry in the boot menu. Confirm with: uname -r"
        ;;
esac
