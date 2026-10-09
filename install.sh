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
  *)
    echo "❌ Error: WattWarden only supports Linux and macOS." >&2
    exit 1
    ;;
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

  ALT_ARCH_NAME=""
  if [[ "${ARCH_NAME}" == "x86_64" ]]; then
    ALT_ARCH_NAME="amd64"
  elif [[ "${ARCH_NAME}" == "aarch64" ]]; then
    ALT_ARCH_NAME="arm64"
  fi

  CANDIDATE_URLS=()
  if [[ -n "${DOWNLOAD_URL}" ]]; then
    CANDIDATE_URLS+=("${DOWNLOAD_URL}")
  else
    CANDIDATE_URLS+=(
      "https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}"
      "https://github.com/${REPO}/releases/download/v2.0.0/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}"
    )
    if [[ -n "${ALT_ARCH_NAME}" ]]; then
      CANDIDATE_URLS+=(
        "https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ALT_ARCH_NAME}"
        "https://github.com/${REPO}/releases/download/v2.0.0/${BINARY_NAME}-${OS_NAME}-${ALT_ARCH_NAME}"
      )
    fi
  fi

  TARGET_DOWNLOAD="${TMP_DIR}/${BINARY_NAME}"
  echo "📥 Fetching precompiled release binary for ${OS_NAME}-${ARCH_NAME}..."
  DOWNLOAD_SUCCESS=0

  for URL in "${CANDIDATE_URLS[@]}"; do
    if command -v curl >/dev/null 2>&1; then
      if curl -fsSL "${URL}" -o "${TARGET_DOWNLOAD}" 2>/dev/null && [[ -s "${TARGET_DOWNLOAD}" ]]; then
        DOWNLOAD_SUCCESS=1
        break
      fi
    elif command -v wget >/dev/null 2>&1; then
      if wget -qO "${TARGET_DOWNLOAD}" "${URL}" 2>/dev/null && [[ -s "${TARGET_DOWNLOAD}" ]]; then
        DOWNLOAD_SUCCESS=1
        break
      fi
    fi
  done

  # Step 4: If precompiled asset is acquired, use it
  if [[ $DOWNLOAD_SUCCESS -eq 1 && -s "${TARGET_DOWNLOAD}" ]]; then
    chmod +x "${TARGET_DOWNLOAD}"
    SRC_BIN="${TARGET_DOWNLOAD}"
  else
    # Locate existing Cargo binary across system and user profiles
    CARGO_BIN=""
    if command -v cargo >/dev/null 2>&1; then
      CARGO_BIN="cargo"
    elif [[ -n "${SUDO_USER}" && -x "/home/${SUDO_USER}/.cargo/bin/cargo" ]]; then
      CARGO_BIN="/home/${SUDO_USER}/.cargo/bin/cargo"
    elif [[ -x "${HOME}/.cargo/bin/cargo" ]]; then
      CARGO_BIN="${HOME}/.cargo/bin/cargo"
    elif [[ -x "/root/.cargo/bin/cargo" ]]; then
      CARGO_BIN="/root/.cargo/bin/cargo"
    fi

    BRANCH="${BRANCH:-master}"

    if [[ -n "${CARGO_BIN}" ]]; then
      echo "⚠️  Precompiled asset not found. Compiling from repository via existing Cargo..."
      "${CARGO_BIN}" install --git "https://github.com/${REPO}.git" --branch "${BRANCH}" wattwarden-cli --root "${TMP_DIR}"
      SRC_BIN="${TMP_DIR}/bin/${BINARY_NAME}"
    elif command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1; then
      echo "⚠️  Precompiled asset unavailable and 'cargo' toolchain not detected."
      echo "⚡ Bootstrapping minimal Rust compiler in temporary environment..."
      export RUSTUP_HOME="${TMP_DIR}/.rustup"
      export CARGO_HOME="${TMP_DIR}/.cargo"
      if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable
      else
        wget -qO- https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable
      fi
      BOOTSTRAP_CARGO="${CARGO_HOME}/bin/cargo"
      if [[ -x "${BOOTSTRAP_CARGO}" ]]; then
        echo "🔨 Compiling WattWarden from repository via Cargo..."
        "${BOOTSTRAP_CARGO}" install --git "https://github.com/${REPO}.git" --branch "${BRANCH}" wattwarden-cli --root "${TMP_DIR}"
        SRC_BIN="${TMP_DIR}/bin/${BINARY_NAME}"
      else
        echo "❌ Error: Could not bootstrap Rust toolchain."
        exit 1
      fi
    else
      echo "❌ Error: Could not download precompiled binary and no build tools available."
      exit 1
    fi
  fi
fi

# Install binary to target directory
echo "🚚 Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
$SUDO mkdir -p "${INSTALL_DIR}"
if command -v install >/dev/null 2>&1; then
  $SUDO install -m 755 "${SRC_BIN}" "${INSTALL_DIR}/${BINARY_NAME}"
else
  $SUDO cp -f "${SRC_BIN}" "${INSTALL_DIR}/${BINARY_NAME}"
  $SUDO chmod 755 "${INSTALL_DIR}/${BINARY_NAME}"
fi

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
