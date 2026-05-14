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
#   1. Optionally install system dependencies (libvirt/qemu on Linux, homebrew packages on macOS).
#   2. Download the latest icbm binary from GitHub releases.
#   3. Run the benchmark.

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
BIN_DIR="$CLONE_DIR/bin"

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
# Detect platform and binary download functions
# ---------------------------------------------------------------------------

detect_os_arch() {
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "$OS" in
    Linux)
      OS_NAME="linux"
      ;;
    Darwin)
      OS_NAME="macos"
      ;;
    *)
      err "Unsupported OS: $OS"
      ;;
  esac

  case "$ARCH" in
    x86_64)
      ARCH_NAME="x86_64"
      ;;
    aarch64|arm64)
      ARCH_NAME="aarch64"
      ;;
    *)
      err "Unsupported architecture: $ARCH"
      ;;
  esac
}

download_binary() {
  detect_os_arch
  
  info "Detecting latest release …"
  
  # Get the latest release info
  RELEASE_JSON=$(curl -sSf "$REPO/releases/latest" 2>/dev/null | head -c 1000000)
  
  # Extract download URL for the current platform
  ASSET_NAME="icbm-$OS_NAME-$ARCH_NAME"
  DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o "\"browser_download_url\":\"[^\"]*$ASSET_NAME[^\"]*\"" | head -1 | cut -d'"' -f4)
  
  if [ -z "$DOWNLOAD_URL" ]; then
    err "Could not find a release binary for $OS_NAME-$ARCH_NAME. Check available releases at $REPO/releases"
  fi
  
  mkdir -p "$BIN_DIR"
  ICBM_BIN="$BIN_DIR/icbm"
  
  if [ -f "$ICBM_BIN" ]; then
    # Check if we already have the latest version
    info "Downloading latest icbm binary …"
  else
    info "Downloading icbm binary …"
  fi
  
  curl -sSfL "$DOWNLOAD_URL" -o "$ICBM_BIN"
  chmod +x "$ICBM_BIN"
  
  ok "Binary ready at $ICBM_BIN"
}

# ---------------------------------------------------------------------------
# 1. Download the latest binary
# ---------------------------------------------------------------------------

download_binary

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
# 3. Run the benchmark
# ---------------------------------------------------------------------------

info "Launching benchmark …"
"$ICBM_BIN" run "$@"
