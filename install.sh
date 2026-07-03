#!/usr/bin/env sh
# ICBM – Integrated Container BenchMark
# One-line installer and runner.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
#
# This script will:
#   1. Download the latest icbm binary from GitHub releases.
#   2. On Linux: download the Docker CLI static binary if docker is missing.
#   3. On macOS: download the Docker CLI *and* bootstrap Colima+Lima so
#      containers run without Docker Desktop or admin privileges.
#   4. Run the benchmark or any other icbm subcommand.
#
# Runs entirely without sudo. All downloaded binaries are placed under
# $HOME/.local/share/icbm and prepended to PATH at runtime.

set -eu

REPO="https://github.com/tascord/icbm"
CLONE_DIR="${ICBM_DIR:-$HOME/.local/share/icbm}"
BIN_DIR="$CLONE_DIR/bin"
LIMA_DIR="$CLONE_DIR/lima"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Required tool '$1' is not installed. Please install it and re-run."
    exit 1
  }
}

download() {
  # Wrapper around curl with retry logic and optional auth token.
  local url="$1"
  local out="$2"
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -sSfL --retry 3 --retry-delay 2 -H "Authorization: token $GITHUB_TOKEN" -o "$out" "$url"
  else
    curl -sSfL --retry 3 --retry-delay 2 -o "$out" "$url"
  fi
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
    Linux)     OS_NAME="linux" ;;
    Darwin)    OS_NAME="macos" ;;
    *)         err "Unsupported OS: $OS" ;;
  esac

  case "$ARCH" in
    x86_64|amd64)    ARCH_NAME="x86_64" ;;
    aarch64|arm64) ARCH_NAME="aarch64" ;;
    *)         err "Unsupported architecture: $ARCH" ;;
  esac
}

# ---------------------------------------------------------------------------
# 1. Download the latest icbm binary
# ---------------------------------------------------------------------------

download_binary() {
  detect_os_arch

  info "Detecting latest icbm release …"

  RELEASE_JSON=$(curl -sSfL --retry 3 --retry-delay 2 "https://api.github.com/repos/tascord/icbm/releases/latest")

  # Detect and report GitHub API rate-limiting early.
  if echo "$RELEASE_JSON" | grep -q '"message":"API rate limit exceeded"'; then
    err "GitHub API rate limit exceeded. Set a GITHUB_TOKEN environment variable to avoid this."
  fi

  ASSET_NAME="icbm-$OS_NAME-$ARCH_NAME"
  DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o '"browser_download_url": "[^"]*"' | grep "$ASSET_NAME" | head -1 | sed 's/.*: "//;s/"$//')

  if [ -z "$DOWNLOAD_URL" ]; then
    err "Could not find a release binary for $OS_NAME-$ARCH_NAME. Check available releases at $REPO/releases"
  fi

  mkdir -p "$BIN_DIR"
  ICBM_BIN="$BIN_DIR/icbm"

  info "Downloading icbm binary …"
  download "$DOWNLOAD_URL" "$ICBM_BIN"
  chmod +x "$ICBM_BIN"

  if [ ! -s "$ICBM_BIN" ]; then
    err "Downloaded icbm binary is empty or missing."
  fi

  ok "Binary ready at $ICBM_BIN"
}

# ---------------------------------------------------------------------------
# 2. Ensure Docker CLI is available
# ---------------------------------------------------------------------------

download_docker_cli() {
  detect_os_arch

  if [ "$OS_NAME" = "linux" ]; then
    DOCKER_BASE="https://download.docker.com/linux/static/stable/$ARCH_NAME/"
  else
    DOCKER_BASE="https://download.docker.com/mac/static/stable/$ARCH_NAME/"
  fi

  DOCKER_VERSION=$(curl -sSfL "$DOCKER_BASE" |
    grep -o 'href="[^"]*\.tgz"' |
    grep -v 'rootless' |
    sed 's/href="//;s/"$//' |
    sort -V |
    tail -1 |
    sed 's/\.tgz//')

  if [ -z "$DOCKER_VERSION" ]; then
    err "Could not determine latest Docker CLI version for $OS_NAME/$ARCH_NAME."
  fi

  TGZ_URL="${DOCKER_BASE}${DOCKER_VERSION}.tgz"
  TGZ_PATH="$BIN_DIR/docker.tgz"

  info "Downloading Docker CLI ($OS_NAME/$ARCH_NAME) …"
  download "$TGZ_URL" "$TGZ_PATH"

  tar -xzf "$TGZ_PATH" -C "$BIN_DIR" --strip-components=1 "docker/docker"
  rm -f "$TGZ_PATH"
  chmod +x "$BIN_DIR/docker"

  if [ ! -x "$BIN_DIR/docker" ]; then
    err "Docker CLI extraction failed."
  fi

  ok "Docker CLI downloaded to $BIN_DIR/docker"
}

# ---------------------------------------------------------------------------
# 3. macOS: bootstrap Colima + Lima so docker works without sudo
# ---------------------------------------------------------------------------

download_lima() {
  detect_os_arch

  # Lima uses arm64/x86_64 in asset names
  LIMA_ARCH="$ARCH_NAME"
  case "$ARCH_NAME" in
    aarch64) LIMA_ARCH="arm64" ;;
  esac

  LIMA_RELEASE="$(curl -sSfL --retry 3 --retry-delay 2 "https://api.github.com/repos/lima-vm/lima/releases/latest")"
  if echo "$LIMA_RELEASE" | grep -q '"message":"API rate limit exceeded"'; then
    err "GitHub API rate limit exceeded while fetching Lima release. Set GITHUB_TOKEN to avoid this."
  fi
  LIMA_TAG=$(echo "$LIMA_RELEASE" | grep -o '"tag_name": "[^"]*"' | head -1 | sed 's/.*: "//;s/"$//')
  LIMA_VERSION="${LIMA_TAG#v}"
  LIMA_ASSET="lima-${LIMA_VERSION}-Darwin-${LIMA_ARCH}.tar.gz"
  LIMA_URL=$(echo "$LIMA_RELEASE" | grep -o '"browser_download_url": "[^"]*"' | grep "$LIMA_ASSET" | head -1 | sed 's/.*: "//;s/"$//')

  if [ -z "$LIMA_URL" ]; then
    err "Could not find Lima release asset '$LIMA_ASSET'."
  fi

  mkdir -p "$LIMA_DIR"

  LIMA_TGZ="$LIMA_DIR/lima.tar.gz"
  info "Downloading Lima ($LIMA_TAG) …"
  download "$LIMA_URL" "$LIMA_TGZ"
  tar -xzf "$LIMA_TGZ" -C "$LIMA_DIR" --strip-components=1
  rm -f "$LIMA_TGZ"

  if [ ! -x "$LIMA_DIR/bin/limactl" ]; then
    err "Lima extraction failed — limactl not found."
  fi

  ok "Lima ready at $LIMA_DIR/bin/limactl"
}

download_colima() {
  detect_os_arch

  # Colima uses arm64/x86_64 in asset names
  COLIMA_ARCH="$ARCH_NAME"
  case "$ARCH_NAME" in
    aarch64) COLIMA_ARCH="arm64" ;;
  esac

  COLIMA_RELEASE="$(curl -sSfL --retry 3 --retry-delay 2 "https://api.github.com/repos/abiosoft/colima/releases/latest")"
  if echo "$COLIMA_RELEASE" | grep -q '"message":"API rate limit exceeded"'; then
    err "GitHub API rate limit exceeded while fetching Colima release. Set GITHUB_TOKEN to avoid this."
  fi
  COLIMA_TAG=$(echo "$COLIMA_RELEASE" | grep -o '"tag_name": "[^"]*"' | head -1 | sed 's/.*: "//;s/"$//')
  COLIMA_ASSET="colima-Darwin-${COLIMA_ARCH}"
  COLIMA_URL=$(echo "$COLIMA_RELEASE" | grep -o '"browser_download_url": "[^"]*"' | grep "$COLIMA_ASSET" | head -1 | sed 's/.*: "//;s/"$//')

  if [ -z "$COLIMA_URL" ]; then
    err "Could not find Colima release asset '$COLIMA_ASSET'."
  fi

  info "Downloading Colima ($COLIMA_TAG) …"
  download "$COLIMA_URL" "$BIN_DIR/colima"
  chmod +x "$BIN_DIR/colima"

  if [ ! -x "$BIN_DIR/colima" ]; then
    err "Colima download failed — binary is missing or not executable."
  fi

  ok "Colima ready at $BIN_DIR/colima"
}

ensure_colima() {
  # If the Colima Docker socket already works, keep using it.
  if DOCKER_HOST="unix://$HOME/.colima/default/docker.sock" docker info >/dev/null 2>&1; then
    ok "Colima Docker socket is alive"
    export DOCKER_HOST="unix://$HOME/.colima/default/docker.sock"
    return 0
  fi

  # If limactl is missing, download Lima.
  if ! command -v limactl >/dev/null 2>&1; then
    if [ ! -x "$LIMA_DIR/bin/limactl" ]; then
      download_lima
    fi
  fi

  # If colima binary is missing, download it.
  if ! command -v colima >/dev/null 2>&1; then
    if [ ! -x "$BIN_DIR/colima" ]; then
      download_colima
    fi
  fi

  # Make sure Colima can find limactl.
  # We export PATH right before calling colima, but also set it here.
  export PATH="$BIN_DIR:$LIMA_DIR/bin:$PATH"

  # Start colima (uses Apple Virtualization.framework — no QEMU, no sudo).
  info "Starting Colima VM (first run downloads a VM image — this may take a few minutes) …"
  COLIMA_BIN="$BIN_DIR/colima"
  if command -v colima >/dev/null 2>&1; then
    COLIMA_BIN="$(command -v colima)"
  fi
  "$COLIMA_BIN" start

  # Wait for the Docker socket to appear.
  info "Waiting for Colima Docker socket …"
  i=0
  while [ "$i" -lt 120 ]; do
    if [ -S "$HOME/.colima/default/docker.sock" ]; then
      break
    fi
    sleep 3
    i=$((i + 1))
  done

  if [ ! -S "$HOME/.colima/default/docker.sock" ]; then
    err "Timed out waiting for Colima Docker socket. Check 'colima status' manually."
  fi

  export DOCKER_HOST="unix://$HOME/.colima/default/docker.sock"
  ok "Colima VM ready"
}

# ---------------------------------------------------------------------------
# 4. Top-level runtime check
# ---------------------------------------------------------------------------

ensure_runtime() {
  # First, do we already have a working Docker?
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    ok "docker daemon reachable"
    return 0
  fi

  # No working Docker. Download the CLI first.
  if ! command -v docker >/dev/null 2>&1; then
    download_docker_cli
  fi

  detect_os_arch

  if [ "$OS_NAME" = "macos" ]; then
    ensure_colima
    return 0
  fi

  # Linux: CLI is downloaded but we can't start the daemon ourselves.
  echo ""
  echo "  ℹ️  Docker CLI downloaded to $BIN_DIR/docker"
  echo "     A Docker daemon is still required. Options:"
  echo "       • Rootless Docker (https://docs.docker.com/engine/security/rootless/)"
  echo "       • Docker Desktop / docker-ce (add your user to the 'docker' group)"
  echo ""
  if ! "$BIN_DIR/docker" info >/dev/null 2>&1; then
    err "Docker daemon is not accessible."
  fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

# 1. Download icbm binary
download_binary

# 2. Determine command — default is 'run'
COMMAND="run"
for arg in "$@"; do
  case "$arg" in
    -*) ;;
    host-info|version|help|--help|--version)
      COMMAND="$arg"
      break
      ;;
    run)
      COMMAND="run"
      break
      ;;
    *)
      COMMAND="run"
      break
      ;;
  esac
done

# 3. Only ensure a container runtime when actually running benchmarks.
if [ "$COMMAND" = "run" ]; then
  ensure_runtime
fi

# 4. Make sure our private bin directories are first on PATH.
export PATH="$BIN_DIR:$LIMA_DIR/bin:$PATH"

# 5. Run icbm, passing through any user-supplied arguments.
info "Launching icbm …"
"$BIN_DIR/icbm" "$@"
