#!/usr/bin/env bash
#
# =============================================================================
#  ZFS_BESKAR_KEY Bootstrap Installer
#  This script forges your system's first Beskar key: the hardware token,
#  configuration, and systemd integration required to protect and unlock your
#  encrypted ZFS pool.
#
#  Run as root:
#  curl -fsSL https://raw.githubusercontent.com/x4ngus/zfs_beskar_key/main/scripts/bootstrap.sh | sudo bash
#
# =============================================================================

set -euo pipefail

BESKAR_LABEL="BESKARKEY"
CONFIG_PATH="/etc/zfs-beskar.toml"
MOUNT_DIR="/mnt/beskar"
RUN_DIR="/run/beskar"
BINARY="/usr/local/bin/zfs_beskar_key"

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Initiating Beskar forge sequence..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

# -----------------------------------------------------------------------------
# 1. Environment validation
# -----------------------------------------------------------------------------
echo "[1/6] Checking environment integrity..."

if [[ $EUID -ne 0 ]]; then
    echo "⛔ This script must be executed as root."
    exit 1
fi

for cmd in parted mkfs.ext4 blkid; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "⛔ Required utility missing: $cmd"
        exit 1
    fi
done

if [[ ! -x "$BINARY" ]]; then
    echo "⛔ $BINARY not found or not executable. Build and install it first."
    exit 1
fi

echo "✅ Environment verified — forge is ready."
echo

# -----------------------------------------------------------------------------
# 2. Select the target USB
# -----------------------------------------------------------------------------
echo "[2/6] Scanning for removable drives..."
lsblk -o NAME,SIZE,TYPE,MOUNTPOINT | grep -E "disk|part" || true

read -rp "Enter the device path for your USB (e.g., /dev/sdb): " DEVICE
if [[ -z "$DEVICE" || ! -b "$DEVICE" ]]; then
    echo "⛔ Invalid device specified."
    exit 1
fi

read -rp "⚠️  This will ERASE all data on $DEVICE. Proceed? (y/N): " CONFIRM
[[ "${CONFIRM,,}" == "y" ]] || exit 1

# -----------------------------------------------------------------------------
# 3. Format and prepare the token
# -----------------------------------------------------------------------------
echo
echo "[3/6] Forging storage medium..."
umount "${DEVICE}"* 2>/dev/null || true
parted -s "$DEVICE" mklabel gpt
parted -s "$DEVICE" mkpart "$BESKAR_LABEL" ext4 1MiB 100%
sleep 1
mkfs.ext4 -F -L "$BESKAR_LABEL" "${DEVICE}1" >/dev/null
echo "✅ Medium aligned and labeled as $BESKAR_LABEL."
echo

# -----------------------------------------------------------------------------
# 4. Generate the Beskar key
# -----------------------------------------------------------------------------
echo "[4/6] Forging encryption key..."
mkdir -p "$MOUNT_DIR"
mount "/dev/disk/by-label/$BESKAR_LABEL" "$MOUNT_DIR"

"$BINARY" forge-key | tee "$MOUNT_DIR/rpool.keyhex" >/dev/null
chmod 0400 "$MOUNT_DIR/rpool.keyhex"
sync
umount "$MOUNT_DIR"
echo "✅ Key forged and stored on USB token."
echo

# -----------------------------------------------------------------------------
# 5. Write system configuration
# -----------------------------------------------------------------------------
echo "[5/6] Writing configuration to $CONFIG_PATH..."
mkdir -p "$(dirname "$CONFIG_PATH")"

cat >"$CONFIG_PATH" <<'EOF'
[policy]
datasets = ["rpool/ROOT"]
zfs_path = "/sbin/zfs"
allow_root = true

[crypto]
timeout_secs = 10

[usb]
key_hex_path = "/run/beskar/rpool.keyhex"

[fallback]
enabled = true
askpass = true
askpass_path = "/usr/bin/systemd-ask-password"
EOF

chmod 0600 "$CONFIG_PATH"
echo "✅ Configuration file written."
echo

# -----------------------------------------------------------------------------
# 6. Deploy systemd units
# -----------------------------------------------------------------------------
echo "[6/6] Integrating with systemd..."
"$BINARY" install-units --config="$CONFIG_PATH"
echo "✅ System units deployed and enabled."
echo

# -----------------------------------------------------------------------------
# Epilogue
# -----------------------------------------------------------------------------
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🛡️  Beskar key successfully forged."
echo "    • USB token label : $BESKAR_LABEL"
echo "    • Config path     : $CONFIG_PATH"
echo "    • Units installed : beskar-usb.mount / beskar-unlock.service"
echo
echo "Insert the key and reboot to test automated pool unlock."
echo "If the token is absent, fallback passphrase protection remains active."
echo
echo "This is the Way."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
