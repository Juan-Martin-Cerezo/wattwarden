#!/usr/bin/env bash
# WattWarden universal installer — one command, every Linux and macOS:
#
#   curl -fsSL https://raw.githubusercontent.com/Juan-Martin-Cerezo/wattwarden/master/install.sh | bash
#
# Bash 3.2 compatible (the macOS stock shell). No distro-specific tools:
# curl/wget for download, cargo only as a last-resort fallback.
set -e

REPO="Juan-Martin-Cerezo/wattwarden"
BINARY_NAME="wattwarden"
INSTALL_DIR="${INSTALL_DIR:-}"

# Detect if executed via local file or piped from curl/wget
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd)"
else
  SCRIPT_DIR=""
fi

echo "⚡ WattWarden Universal Installer"
echo "──────────────────────────────────────────"

# Detect OS
OS="$(uname -s)"
case "${OS}" in
  Linux*)  OS_NAME=linux ;;
  Darwin*) OS_NAME=macos ;;
  *)
    echo "❌ Error: WattWarden only supports Linux and macOS (got ${OS})." >&2
    exit 1
    ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64*|amd64*)  ARCH_NAME=x86_64 ;;
  aarch64*|arm64*) ARCH_NAME=aarch64 ;;
  *)
    echo "❌ Error: unsupported architecture '${ARCH}' (need x86_64 or aarch64)." >&2
    exit 1
    ;;
esac

# Install location: prefer the system dir, Homebrew's prefix on Apple Silicon
# when it is already on PATH, else a user dir when no escalator is available.
if [ -z "${INSTALL_DIR}" ]; then
  if [ "${OS_NAME}" = "macos" ] && [ -d /opt/homebrew/bin ] && echo ":${PATH}:" | grep -q ":/opt/homebrew/bin:"; then
    INSTALL_DIR="/opt/homebrew/bin"
  else
    INSTALL_DIR="/usr/local/bin"
  fi
fi

# Determine privilege escalator
SUDO=""
if [ "$(id -u)" -ne 0 ] && [ ! -w "${INSTALL_DIR}" ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  elif command -v doas >/dev/null 2>&1; then
    SUDO="doas"
  else
    INSTALL_DIR="${HOME}/.local/bin"
    echo "ℹ️  No sudo/doas found — installing to ${INSTALL_DIR}"
  fi
fi

SRC_BIN=""
TMP_DIR=""
cleanup() { [ -n "${TMP_DIR}" ] && rm -rf "${TMP_DIR}"; }
trap cleanup EXIT

fetch() {
  # fetch <url> <dest>
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2" 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$2" "$1" 2>/dev/null
  else
    return 1
  fi
}

# Step 1: a locally compiled release binary
if [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/target/release/${BINARY_NAME}" ]; then
  echo "📦 Found locally compiled release binary."
  SRC_BIN="${SCRIPT_DIR}/target/release/${BINARY_NAME}"

# Step 2: source checkout with Cargo available -> auto-compile
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
  echo "🔨 Compiling WattWarden release binary via Cargo..."
  cargo build --workspace --release --manifest-path "${SCRIPT_DIR}/Cargo.toml"
  SRC_BIN="${SCRIPT_DIR}/target/release/${BINARY_NAME}"

# Step 3: precompiled multi-arch binary from GitHub Releases
else
  TMP_DIR="$(mktemp -d)"

  case "${ARCH_NAME}" in
    x86_64)  ALT_ARCH_NAME="amd64" ;;
    aarch64) ALT_ARCH_NAME="arm64" ;;
  esac

  CANDIDATE_URLS=""
  if [ -n "${DOWNLOAD_URL:-}" ]; then
    CANDIDATE_URLS="${DOWNLOAD_URL}"
  else
    # releases/latest first (always current), then pinned fallbacks.
    CANDIDATE_URLS="https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}"
    for TAG in v2.2.0 v2.1.1 v2.1.0; do
      CANDIDATE_URLS="${CANDIDATE_URLS} https://github.com/${REPO}/releases/download/${TAG}/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}"
    done
    if [ -n "${ALT_ARCH_NAME}" ]; then
      CANDIDATE_URLS="${CANDIDATE_URLS} https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ALT_ARCH_NAME}"
    fi
  fi

  TARGET_DOWNLOAD="${TMP_DIR}/${BINARY_NAME}"
  echo "📥 Fetching precompiled release binary for ${OS_NAME}-${ARCH_NAME}..."
  DOWNLOAD_SUCCESS=0

  for URL in ${CANDIDATE_URLS}; do
    if fetch "${URL}" "${TARGET_DOWNLOAD}" && [ -s "${TARGET_DOWNLOAD}" ]; then
      DOWNLOAD_SUCCESS=1
      break
    fi
  done

  if [ "${DOWNLOAD_SUCCESS}" -eq 1 ]; then
    chmod +x "${TARGET_DOWNLOAD}"
    SRC_BIN="${TARGET_DOWNLOAD}"
  else
    # Last resort: build from source with whatever cargo is available.
    CARGO_BIN=""
    if command -v cargo >/dev/null 2>&1; then
      CARGO_BIN="cargo"
    elif [ -n "${SUDO_USER:-}" ] && [ -x "/home/${SUDO_USER}/.cargo/bin/cargo" ]; then
      CARGO_BIN="/home/${SUDO_USER}/.cargo/bin/cargo"
    elif [ -x "${HOME}/.cargo/bin/cargo" ]; then
      CARGO_BIN="${HOME}/.cargo/bin/cargo"
    elif [ -x "/root/.cargo/bin/cargo" ]; then
      CARGO_BIN="/root/.cargo/bin/cargo"
    fi

    BRANCH="${BRANCH:-master}"

    if [ -n "${CARGO_BIN}" ]; then
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
      if [ -x "${BOOTSTRAP_CARGO}" ]; then
        echo "🔨 Compiling WattWarden from repository via Cargo..."
        "${BOOTSTRAP_CARGO}" install --git "https://github.com/${REPO}.git" --branch "${BRANCH}" wattwarden-cli --root "${TMP_DIR}"
        SRC_BIN="${TMP_DIR}/bin/${BINARY_NAME}"
      else
        echo "❌ Error: could not bootstrap a Rust toolchain." >&2
        exit 1
      fi
    else
      echo "❌ Error: could not download a precompiled binary and no build tools available." >&2
      exit 1
    fi
  fi
fi

if [ -z "${SRC_BIN}" ] || [ ! -f "${SRC_BIN}" ]; then
  echo "❌ Error: no WattWarden binary to install." >&2
  exit 1
fi

# Install binary
echo "🚚 Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
$SUDO mkdir -p "${INSTALL_DIR}"
if command -v install >/dev/null 2>&1; then
  $SUDO install -m 755 "${SRC_BIN}" "${INSTALL_DIR}/${BINARY_NAME}"
else
  $SUDO cp -f "${SRC_BIN}" "${INSTALL_DIR}/${BINARY_NAME}"
  $SUDO chmod 755 "${INSTALL_DIR}/${BINARY_NAME}"
fi

# Background service: systemd where it exists, launchd on macOS; elsewhere the
# detached daemon (`wattwarden --start`) works without any service manager.
if [ "${SKIP_SERVICE:-0}" != "1" ]; then
  if [ "${OS_NAME}" = "linux" ] && command -v systemctl >/dev/null 2>&1; then
    echo "⚙️  Configuring background systemd service..."
    $SUDO "${INSTALL_DIR}/${BINARY_NAME}" --install-service >/dev/null 2>&1 || true
  elif [ "${OS_NAME}" = "macos" ] && command -v launchctl >/dev/null 2>&1; then
    echo "⚙️  Configuring background launchd service..."
    $SUDO "${INSTALL_DIR}/${BINARY_NAME}" --install-service >/dev/null 2>&1 || true
  else
    echo "ℹ️  No systemd/launchd — use 'wattwarden --start' for the background daemon."
  fi
fi

# Verification
echo "──────────────────────────────────────────"
echo "✅ Installation complete!"
echo "   Binary location: ${INSTALL_DIR}/${BINARY_NAME}"
"${INSTALL_DIR}/${BINARY_NAME}" --version || true
echo ""

# PATH hint
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "⚠️  ${INSTALL_DIR} is not on your PATH. Add it:"
    echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc && export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

echo "👉 Run anytime: wattwarden"
