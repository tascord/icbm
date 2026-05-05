#!/usr/bin/env sh
# ICBM – Integrated Container BenchMark
# One-line installer and runner.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
#   curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh -s -- --install-deps
#
# Flags:
#   --install-deps   Automatically install missing system dependencies without prompting.
#
# The script will:
#   1. Check for / install Rust via rustup.
#   2. Optionally install system dependencies (libvirt/qemu on Linux, homebrew packages on macOS).
#   3. Sparse-clone only the tool source from this repository.
#   4. Build icbm in release mode.
#   5. Run the benchmark.

set -eu

# ---------------------------------------------------------------------------
# Parse flags
# ---------------------------------------------------------------------------

AUTO_DEPS=0
for arg in "$@"; do
  case "$arg" in
    --install-deps) AUTO_DEPS=1 ;;
  esac
done

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
# 2. Check / install libvirt + qemu deps
# ---------------------------------------------------------------------------

DEPS_MISSING=0
for tool in virsh virt-install qemu-img; do
  command -v "$tool" >/dev/null 2>&1 || { DEPS_MISSING=1; break; }
done

if [ "$DEPS_MISSING" -eq 1 ]; then
  OS="$(uname -s)"

  install_deps_linux() {
    if command -v apt-get >/dev/null 2>&1; then
      sudo apt-get update
      sudo apt-get install -y qemu-kvm libvirt-daemon-system virtinst
    elif command -v dnf >/dev/null 2>&1; then
      sudo dnf install -y qemu-kvm libvirt virt-install
    elif command -v nix-env >/dev/null 2>&1; then
      nix-env -iA nixpkgs.libvirt nixpkgs.virt-manager nixpkgs.qemu
    else
      err "Could not detect a supported package manager. Please install qemu-kvm, libvirt and virt-install manually."
    fi
  }

  install_deps_macos() {
    if ! command -v brew >/dev/null 2>&1; then
      info "Homebrew not found – installing …"
      /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    fi
    brew install qemu libvirt
    brew services start libvirt
  }

  if [ "$AUTO_DEPS" -eq 1 ]; then
    INSTALL_DEPS=y
  else
    echo ""
    echo "  ⚠️  One or more required tools (virsh, virt-install, qemu-img) were not found."
    if [ "$OS" = "Darwin" ]; then
      echo "  macOS:             brew install qemu libvirt && brew services start libvirt"
    else
      echo "  Debian/Ubuntu:     sudo apt install qemu-kvm libvirt-daemon-system virtinst"
      echo "  Fedora/RHEL:       sudo dnf install qemu-kvm libvirt virt-install"
      echo "  NixOS:             nix-env -iA nixpkgs.libvirt nixpkgs.virt-manager nixpkgs.qemu"
    fi
    echo ""
    printf "  Install dependencies now? [Y/n] "
    read -r INSTALL_DEPS </dev/tty
    INSTALL_DEPS="${INSTALL_DEPS:-y}"
  fi

  case "$INSTALL_DEPS" in
    [Yy]*)
      info "Installing dependencies …"
      if [ "$OS" = "Darwin" ]; then
        install_deps_macos
      else
        install_deps_linux
      fi
      ok "Dependencies installed"
      ;;
    *)
      echo "  Skipping dependency installation. Re-run after installing the required tools."
      exit 1
      ;;
  esac
fi

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
