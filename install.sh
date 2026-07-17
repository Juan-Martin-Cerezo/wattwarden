#!/bin/bash

# Terminate if any error occurs
set -e

VERSION="v1.0.3"
REPO="Juan-Martin-Cerezo/wattwarden"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="wattwarden"

echo "⚡ Starting WattWarden installation..."

# 1. Verify if running as root
if [[ $EUID -ne 0 ]]; then
   echo "❌ Error: This script must be run as root (with sudo)."
   exit 1
fi

# 2. Detect Operating System
OS="$(uname -s)"
case "${OS}" in
  Linux*)     OS_NAME=linux;;
  Darwin*)    OS_NAME=macos;;
  *)          echo "❌ Error: Unsupported OS: ${OS}"; exit 1;;
esac

# 3. Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64*)    ARCH_NAME=amd64;;
  i386*|i686*) ARCH_NAME=386;;
  aarch64*)   ARCH_NAME=arm64;;
  arm64*)     ARCH_NAME=arm64;;
  *)          echo "❌ Error: Unsupported architecture: ${ARCH}"; exit 1;;
esac

echo "🔎 Detected system: ${OS_NAME} (${ARCH_NAME})"

# 4. Build Download URL
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}"

# 5. Download using curl or wget
echo "⬇️ Downloading WattWarden ${VERSION}..."
TMP_FILE="/tmp/${BINARY_NAME}"

if command -v curl >/dev/null 2>&1; then
  curl -fsSL "${DOWNLOAD_URL}" -o "${TMP_FILE}"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "${TMP_FILE}" "${DOWNLOAD_URL}"
else
  echo "❌ Error: Neither 'curl' nor 'wget' was found. Please install one of them."
  exit 1
fi

# 6. Install Binary
echo "📦 Installing to ${INSTALL_DIR}..."
mv "${TMP_FILE}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "✅ Success! WattWarden has been installed."
echo "👉 You can now run it by typing: sudo wattwarden"
