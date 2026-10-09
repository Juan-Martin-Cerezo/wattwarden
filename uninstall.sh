#!/usr/bin/env bash
set -e

INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="wattwarden"
TARGET_BIN="${INSTALL_DIR}/${BINARY_NAME}"
SYSTEMD_UNIT="/etc/systemd/system/wattwarden.service"

echo "🛑 Starting WattWarden uninstallation..."

# Check root or sudo
if [[ $EUID -ne 0 && (! -w "${INSTALL_DIR}" || -f "${SYSTEMD_UNIT}") ]]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "❌ Error: Root or sudo privileges required to uninstall from ${INSTALL_DIR}."
    exit 1
  fi
else
  SUDO=""
fi

# Stop and disable systemd service
if command -v systemctl >/dev/null 2>&1; then
  if systemctl is-active --quiet wattwarden 2>/dev/null; then
    echo "⚙️ Stopping wattwarden service..."
    $SUDO systemctl stop wattwarden 2>/dev/null || true
  fi
  if systemctl is-enabled --quiet wattwarden 2>/dev/null; then
    echo "⚙️ Disabling wattwarden service..."
    $SUDO systemctl disable wattwarden 2>/dev/null || true
  fi
  if [[ -f "${SYSTEMD_UNIT}" ]]; then
    echo "🗑️ Removing systemd unit ${SYSTEMD_UNIT}..."
    $SUDO rm -f "${SYSTEMD_UNIT}"
    $SUDO systemctl daemon-reload 2>/dev/null || true
  fi
fi

# Remove binary
if [[ -f "${TARGET_BIN}" ]]; then
  echo "🗑️ Removing binary ${TARGET_BIN}..."
  $SUDO rm -f "${TARGET_BIN}"
fi

echo "✅ WattWarden uninstalled successfully."
