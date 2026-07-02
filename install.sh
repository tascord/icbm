#!/usr/bin/env sh
# ICBM – Integrated Container BenchMark
# One-line installer and runner.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
#
# This script will:
#   1. Download the latest icbm binary from GitHub releases.
#   2. Download Docker CLI static binary if docker is not found (Linux only).
#   3. Run the benchmark.
#
# Runs entirely without sudo. All downloaded binaries are placed under
# $HOME/.local/share/icbm/bin and prepend to PATH at runtime.

set -eu

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
# Detect platform
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

# ---------------------------------------------------------------------------
# 1. Download the latest icbm binary
# ---------------------------------------------------------------------------

download_binary() {
  detect_os_arch

  info "Detecting latest release …"

  RELEASE_JSON=$(curl -sSfL "https://api.github.com/repos/tascord/icbm/releases/latest")

  ASSET_NAME="icbm-$OS_NAME-$ARCH_NAME"
  DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o '"browser_download_url": "[^"]*"' | grep "$ASSET_NAME" | head -1 | sed 's/.*: "//;s/"$//')

  if [ -z "$DOWNLOAD_URL" ]; then
    err "Could not find a release binary for $OS_NAME-$ARCH_NAME. Check available releases at $REPO/releases"
  fi

  mkdir -p "$BIN_DIR"
  ICBM_BIN="$BIN_DIR/icbm"

  info "Downloading icbm binary …"
  curl -sSfL "$DOWNLOAD_URL" -o "$ICBM_BIN"
  chmod +x "$ICBM_BIN"

  ok "Binary ready at $ICBM_BIN"
}

# ---------------------------------------------------------------------------
# 2. Ensure Docker CLI is available (Linux only)
# ---------------------------------------------------------------------------

ensure_docker() {
  if command -v docker >/dev/null 2>&1; then
    ok "docker found in PATH"
    return 0
  fi

  detect_os_arch

  if [ "$OS_NAME" = "macos" ]; then
    echo ""
    echo "  ⚠️  Docker was not found."
    echo "  On macOS, install Docker Desktop (https://www.docker.com/products/docker-desktop/)"
    echo "  or Colima (https://github.com/abiosoft/colima) and make sure 'docker' is in your PATH."
    echo ""
    err "Docker is required to run icbm."
  fi

  info "Docker not found; downloading static Docker CLI binary …"

  # Map our architecture names to Docker's published archive names
  DOCKER_ARCH="$ARCH_NAME"
  case "$ARCH_NAME" in
    x86_64) DOCKER_ARCH="x86_64" ;;
    aarch64) DOCKER_ARCH="aarch64" ;;
  esac

  # Query Docker's API for the latest stable version tag
  DOCKER_VERSION=$(curl -sSfL "https://download.docker.com/linux/static/stable/$DOCKER_ARCH/" |
    grep -o 'href="[^"]*\.tgz"' |
    grep -v 'rootless' |
    sed 's/href="//;s/"$//' |
    sort -V |
    tail -1 |
    sed 's/\.tgz//')

  if [ -z "$DOCKER_VERSION" ]; then
    err "Could not determine latest Docker CLI version for $DOCKER_ARCH."
  fi

  TGZ_URL="https://download.docker.com/linux/static/stable/$DOCKER_ARCH/${DOCKER_VERSION}.tgz"
  TGZ_PATH="$BIN_DIR/docker.tgz"

  curl -sSfL "$TGZ_URL" -o "$TGZ_PATH"

  # Extract only the docker binary from the tarball
  tar -xzf "$TGZ_PATH" -C "$BIN_DIR" --strip-components=1 "docker/docker"
  rm -f "$TGZ_PATH"
  chmod +x "$BIN_DIR/docker"

  ok "Docker CLI downloaded to $BIN_DIR/docker"
  echo ""
  echo "  ℹ️  Make sure you have a Docker daemon running and that your user"
  echo "     is in the 'docker' group (or the socket is otherwise accessible)."
  echo ""
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

# 1. Download icbm binary
download_binary

# 2. Ensure Docker is available (downloads static binary on Linux if missing)
ensure_docker

# 3. Make sure our private bin directory is first on PATH so the downloaded
#    docker binary is found even if the user didn't have one globally.
export PATH="$BIN_DIR:$PATH"

# 4. Run the benchmark
info "Launching benchmark …"
"$BIN_DIR/icbm" run --provider "$RUN_PROVIDER" "$@"
