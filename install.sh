#!/bin/bash

# Terminate if any error occurs
set -e

REPO="Juan-Martin-Cerezo/wattwarden"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="wattwarden"

echo "⚡ Starting WattWarden installation..."

# 1. Detect Operating System
OS="$(uname -s)"
case "${OS}" in
  Linux*)     OS_NAME=linux;;
  Darwin*)    OS_NAME=macos;;
  *)          echo "❌ Error: Unsupported OS: ${OS}"; exit 1;;
esac

# 2. Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64*)    ARCH_NAME=amd64;;
  i386*|i686*) ARCH_NAME=386;;
  aarch64*)   ARCH_NAME=arm64;;
  arm64*)     ARCH_NAME=arm64;;
  *)          echo "❌ Error: Unsupported architecture: ${ARCH}"; exit 1;;
esac

echo "🔎 Detected system: ${OS_NAME} (${ARCH_NAME})"

# 3. Build the latest GitHub release URL
DOWNLOAD_URL="${DOWNLOAD_URL:-https://github.com/${REPO}/releases/latest/download/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}}"
TMP_FILE="$(mktemp)"
trap 'rm -f "${TMP_FILE}"' EXIT

# 4. Download using curl or wget
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_FILE}"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "${TMP_FILE}" "${DOWNLOAD_URL}"
else
  echo "❌ Error: Neither 'curl' nor 'wget' was found. Please install one of them."
  exit 1
fi

# 5. Install the binary with elevated permissions when needed
if [[ $EUID -eq 0 || -w "${INSTALL_DIR}" ]]; then
  install -Dm755 "${TMP_FILE}" "${INSTALL_DIR}/${BINARY_NAME}"
elif command -v sudo >/dev/null 2>&1; then
  sudo install -Dm755 "${TMP_FILE}" "${INSTALL_DIR}/${BINARY_NAME}"
else
  echo "❌ Error: Root privileges or sudo are required to install to ${INSTALL_DIR}."
  exit 1
fi

echo "✅ Success! WattWarden has been installed."
echo "👉 You can now run it by typing: sudo wattwarden"
