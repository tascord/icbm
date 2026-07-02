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
#   1. Optionally install Docker (preferred) or libvirt/qemu (Linux fallback).
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
RUN_PROVIDER="auto"

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

  if [ "$OS_NAME" = "macos" ] && [ "$ARCH_NAME" = "x86_64" ]; then
    err "macOS Intel (x86_64) is not supported. Only Apple Silicon (aarch64) is supported."
  fi
}

download_binary() {
  detect_os_arch

  info "Detecting latest release …"

  # Get the latest release info from the GitHub API
  RELEASE_JSON=$(curl -sSfL "https://api.github.com/repos/tascord/icbm/releases/latest")

  # Extract download URL for the current platform
  ASSET_NAME="icbm-$OS_NAME-$ARCH_NAME"
  DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o "\"browser_download_url\": \"[^\"]*$ASSET_NAME\"" | head -1 | cut -d'"' -f4)

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
# 2. Check / install VM provider deps
# ---------------------------------------------------------------------------

OS="$(uname -s)"
DEPS_MISSING=0

# Docker is the preferred provider on all platforms.
# On Linux, libvirt tools serve as a fallback when Docker is unavailable.
if [ "$OS" = "Darwin" ]; then
  if ! command -v docker >/dev/null 2>&1; then
    DEPS_MISSING=1
  fi
else
  if ! command -v docker >/dev/null 2>&1; then
    DEPS_MISSING=1
  fi

  # Also check libvirt fallback tools.
  for tool in virsh virt-install qemu-img; do
    command -v "$tool" >/dev/null 2>&1 || { LIBVIRT_MISSING=1; break; }
  done
fi

if [ "$DEPS_MISSING" -eq 1 ]; then
  install_deps_linux() {
    if command -v apt-get >/dev/null 2>&1; then
      sudo apt-get update
      sudo apt-get install -y docker.io
      # Also ensure libvirt fallback tools are present.
      sudo apt-get install -y qemu-kvm libvirt-daemon-system virtinst
    elif command -v dnf >/dev/null 2>&1; then
      sudo dnf install -y docker
      # Also ensure libvirt fallback tools are present.
      sudo dnf install -y qemu-kvm libvirt virt-install
    elif command -v nix-env >/dev/null 2>&1; then
      nix-env -iA nixpkgs.docker
      nix-env -iA nixpkgs.libvirt nixpkgs.virt-manager nixpkgs.qemu
    else
      err "Could not detect a supported package manager. Please install docker (and libvirt tools as fallback) manually."
    fi
  }

  install_deps_macos() {
    if ! command -v brew >/dev/null 2>&1; then
      info "Homebrew not found – installing …"
      NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    fi

    # colima + docker CLI: runs entirely in user-space (no sudo required).
    # Docker Desktop (--cask docker) requires admin privileges and a system
    # extension, so we avoid it here.
    brew install colima docker

    # Start a colima VM with sensible defaults (2 vCPUs, 4 GB RAM, 60 GB disk).
    # These are intentionally modest so the benchmark runs on most Apple Silicon
    # machines; adjust with `colima stop && colima start --cpu N --memory N` if needed.
    colima start --cpu 2 --memory 4 --disk 60 || \
      err "colima failed to start. Ensure virtualization is supported and re-run, or start it manually with: colima start"
  }

  if [ "$AUTO_DEPS" -eq 1 ]; then
    INSTALL_DEPS=y
  else
    echo ""
    if [ "$OS" = "Darwin" ]; then
      echo "  ⚠️  Docker was not found."
      echo "  macOS (Apple Silicon):  brew install colima docker && colima start"
    else
      echo "  ⚠️  Docker was not found."
      echo "  Debian/Ubuntu:     sudo apt install docker.io"
      echo "  Fedora/RHEL:       sudo dnf install docker"
      echo ""
      echo "  libvirt fallback tools will also be installed in case you prefer --provider libvirt."
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

if command -v docker >/dev/null 2>&1; then
  ok "docker found"
else
  ok "libvirt fallback tools found"
fi

# ---------------------------------------------------------------------------
# 3. Run the benchmark
# ---------------------------------------------------------------------------

info "Launching benchmark …"
"$ICBM_BIN" run --provider "$RUN_PROVIDER" "$@"
