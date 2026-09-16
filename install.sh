#!/usr/bin/env bash
set -e

REPO="Juan-Martin-Cerezo/wattwarden"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="wattwarden"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd)"

echo "⚡ Starting WattWarden installation..."

# Check root or sudo
if [[ $EUID -ne 0 && ! -w "${INSTALL_DIR}" ]]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "❌ Error: Root or sudo privileges required to install to ${INSTALL_DIR}."
    exit 1
  fi
else
  SUDO=""
fi

# Detect OS & Arch
OS="$(uname -s)"
case "${OS}" in
  Linux*)  OS_NAME=linux ;;
  Darwin*) OS_NAME=macos ;;
  *) echo "❌ Unsupported OS: ${OS}"; exit 1 ;;
esac

ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64*) ARCH_NAME=x86_64 ;;
  aarch64*|arm64*) ARCH_NAME=aarch64 ;;
  *) echo "❌ Unsupported architecture: ${ARCH}"; exit 1 ;;
esac

# 1. Prefer locally built release binary if inside repository
if [[ -f "${SCRIPT_DIR}/target/release/${BINARY_NAME}" ]]; then
  echo "📦 Using locally compiled release binary."
  SRC_BIN="${SCRIPT_DIR}/target/release/${BINARY_NAME}"
else
  # 2. Otherwise download latest release asset from GitHub
  TMP_FILE="$(mktemp)"
  trap 'rm -f "${TMP_FILE}"' EXIT
  DOWNLOAD_URL="${DOWNLOAD_URL:-https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}}"
  echo "📥 Fetching binary from: ${DOWNLOAD_URL}"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_FILE}"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "${TMP_FILE}" "${DOWNLOAD_URL}"
  else
    echo "❌ Error: Neither curl nor wget found."
    exit 1
  fi
  SRC_BIN="${TMP_FILE}"
fi

# Install binary to /usr/local/bin
$SUDO mkdir -p "${INSTALL_DIR}"
$SUDO cp -f "${SRC_BIN}" "${INSTALL_DIR}/${BINARY_NAME}"
$SUDO chmod 755 "${INSTALL_DIR}/${BINARY_NAME}"

# Optional systemd service enable
if [[ "${SKIP_SERVICE:-0}" != "1" && "${OS_NAME}" == "linux" ]]; then
  echo "⚙️ Configuring background systemd service..."
  $SUDO "${INSTALL_DIR}/${BINARY_NAME}" service install >/dev/null 2>&1 || true
fi

echo "✅ Success! WattWarden installed to ${INSTALL_DIR}/${BINARY_NAME}"
echo "👉 Run from anywhere: sudo wattwarden"
