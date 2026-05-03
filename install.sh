#!/usr/bin/env sh
# ICBM – Integrated Container BenchMark
# One-line installer and runner.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
#
# The script will:
#   1. Check for / install Rust via rustup.
#   2. Sparse-clone only the tool source from this repository.
#   3. Build icbm in release mode.
#   4. Run the benchmark.

set -eu

REPO="https://github.com/tascord/icbm"
CLONE_DIR="${ICBM_DIR:-$HOME/.local/share/icbm}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Required tool '$1' is not installed. Please install it and re-run."
    exit 1
  }
}

info()  { printf '\033[1;36m==> \033[0m%s\n' "$*"; }
ok()    { printf '\033[1;32m ✓  \033[0m%s\n' "$*"; }
err()   { printf '\033[1;31m ✗  \033[0m%s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# 1. Ensure Rust is available
# ---------------------------------------------------------------------------

if ! command -v cargo >/dev/null 2>&1; then
  info "Installing Rust via rustup …"
  curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
  ok "Rust installed"
else
  ok "Rust already installed: $(rustc --version)"
fi

# shellcheck disable=SC1091
. "$HOME/.cargo/env" 2>/dev/null || true

# ---------------------------------------------------------------------------
# 2. Check libvirt / virsh
# ---------------------------------------------------------------------------

for tool in virsh virt-install qemu-img; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo ""
    echo "  ⚠️  '$tool' not found."
    echo "  On Debian/Ubuntu:  sudo apt install qemu-kvm libvirt-daemon-system virtinst"
    echo "  On NixOS:          nix-env -iA nixpkgs.libvirt nixpkgs.virt-manager"
    echo "  On Fedora/RHEL:    sudo dnf install qemu-kvm libvirt virt-install"
    echo ""
    echo "  After installing, re-run this script."
    exit 1
  fi
done

ok "libvirt/virsh found"

# ---------------------------------------------------------------------------
# 3. Sparse-clone the repo (tool source only — skip workspace/ and .github/)
# ---------------------------------------------------------------------------

if [ -d "$CLONE_DIR/.git" ]; then
  info "Updating existing clone …"
  git -C "$CLONE_DIR" fetch --depth=1 origin main
  git -C "$CLONE_DIR" checkout FETCH_HEAD
else
  info "Sparse-cloning icbm …"
  mkdir -p "$CLONE_DIR"
  git clone --filter=blob:none --sparse --depth=1 "$REPO" "$CLONE_DIR"
  git -C "$CLONE_DIR" sparse-checkout set src Cargo.toml Cargo.lock
fi

ok "Source ready in $CLONE_DIR"

# ---------------------------------------------------------------------------
# 4. Build icbm (release)
# ---------------------------------------------------------------------------

info "Building icbm (release) …"
cargo build --release --manifest-path "$CLONE_DIR/Cargo.toml"
ok "Build complete"

ICBM_BIN="$CLONE_DIR/target/release/icbm"

# ---------------------------------------------------------------------------
# 5. Run the benchmark
# ---------------------------------------------------------------------------

info "Launching benchmark …"
"$ICBM_BIN" run "$@"
