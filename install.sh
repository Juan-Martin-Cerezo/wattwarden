#!/usr/bin/env bash
set -e

REPO="Juan-Martin-Cerezo/wattwarden"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="wattwarden"

# Detect if executed via local file or piped from curl/wget
if [[ -n "${BASH_SOURCE[0]}" && -f "${BASH_SOURCE[0]}" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd)"
else
  SCRIPT_DIR=""
fi

echo "⚡ WattWarden Universal Installer"
echo "──────────────────────────────────────────"

# Determine privilege escalator
if [[ $EUID -ne 0 && ! -w "${INSTALL_DIR}" ]]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  elif command -v doas >/dev/null 2>&1; then
    SUDO="doas"
  else
    # Fallback to user-local directory if no root escalator available
    INSTALL_DIR="${HOME}/.local/bin"
    SUDO=""
  fi
else
  SUDO=""
fi

# Detect OS
OS="$(uname -s)"
case "${OS}" in
  Linux*)  OS_NAME=linux ;;
  Darwin*) OS_NAME=macos ;;
  MINGW*|MSYS*|CYGWIN*) OS_NAME=windows ;;
  *)       OS_NAME=fallback ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64*|amd64*)  ARCH_NAME=x86_64 ;;
  aarch64*|arm64*) ARCH_NAME=aarch64 ;;
  *)               ARCH_NAME=unknown ;;
esac

SRC_BIN=""
CLEANUP_TMP=0

# Step 1: Check if pre-compiled release binary already exists locally
if [[ -n "${SCRIPT_DIR}" && -f "${SCRIPT_DIR}/target/release/${BINARY_NAME}" ]]; then
  echo "📦 Found locally compiled release binary."
  SRC_BIN="${SCRIPT_DIR}/target/release/${BINARY_NAME}"

# Step 2: Check if inside a source checkout with Cargo available -> Auto-compile
elif [[ -n "${SCRIPT_DIR}" && -f "${SCRIPT_DIR}/Cargo.toml" ]] && command -v cargo >/dev/null 2>&1; then
  echo "🔨 Compiling WattWarden release binary via Cargo..."
  cargo build --workspace --release --manifest-path "${SCRIPT_DIR}/Cargo.toml"
  SRC_BIN="${SCRIPT_DIR}/target/release/${BINARY_NAME}"

# Step 3: Fetch precompiled multi-arch binary from GitHub Releases
else
  TMP_DIR="$(mktemp -d)"
  CLEANUP_TMP=1
  trap '[[ $CLEANUP_TMP -eq 1 ]] && rm -rf "${TMP_DIR}"' EXIT

  DOWNLOAD_URL="${DOWNLOAD_URL:-https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}}"
  TARGET_DOWNLOAD="${TMP_DIR}/${BINARY_NAME}"

  echo "📥 Fetching precompiled release binary for ${OS_NAME}-${ARCH_NAME}..."
  DOWNLOAD_SUCCESS=0
  if command -v curl >/dev/null 2>&1; then
    if curl -fsSL "${DOWNLOAD_URL}" -o "${TARGET_DOWNLOAD}" 2>/dev/null; then
      DOWNLOAD_SUCCESS=1
    fi
  elif command -v wget >/dev/null 2>&1; then
    if wget -qO "${TARGET_DOWNLOAD}" "${DOWNLOAD_URL}" 2>/dev/null; then
      DOWNLOAD_SUCCESS=1
    fi
  fi

  # Step 4: If precompiled asset is unavailable, fallback to cargo install
  if [[ $DOWNLOAD_SUCCESS -eq 1 && -s "${TARGET_DOWNLOAD}" ]]; then
    chmod +x "${TARGET_DOWNLOAD}"
    SRC_BIN="${TARGET_DOWNLOAD}"
  elif command -v cargo >/dev/null 2>&1; then
    echo "⚠️  Precompiled asset not found. Compiling from repository via Cargo..."
    cargo install --git "https://github.com/${REPO}.git" wattwarden-cli --root "${TMP_DIR}"
    SRC_BIN="${TMP_DIR}/bin/${BINARY_NAME}"
  else
    echo "❌ Error: Could not download precompiled binary and 'cargo' toolchain is not installed."
    echo "   Please install Rust (https://rustup.rs) or download a release binary manually."
    exit 1
  fi
fi

# Install binary to target directory
echo "🚚 Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
$SUDO mkdir -p "${INSTALL_DIR}"
$SUDO cp -f "${SRC_BIN}" "${INSTALL_DIR}/${BINARY_NAME}"
$SUDO chmod 755 "${INSTALL_DIR}/${BINARY_NAME}"

# Configure system background service on Linux
if [[ "${SKIP_SERVICE:-0}" != "1" && "${OS_NAME}" == "linux" ]]; then
  echo "⚙️ Configuring background systemd service..."
  $SUDO "${INSTALL_DIR}/${BINARY_NAME}" service install >/dev/null 2>&1 || true
fi

# Verification
echo "──────────────────────────────────────────"
echo "✅ Installation complete!"
echo "   Binary location: ${INSTALL_DIR}/${BINARY_NAME}"
echo ""
echo "🚀 Verification:"
"${INSTALL_DIR}/${BINARY_NAME}" --version || true
"${INSTALL_DIR}/${BINARY_NAME}" --status || true
echo ""
echo "👉 Run anytime: sudo wattwarden"
