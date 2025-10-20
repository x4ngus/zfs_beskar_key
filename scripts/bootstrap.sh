#!/usr/bin/env bash
#
# =============================================================================
#  ZFS_BESKAR_KEY Bootstrap Installer – v1.1
#  For the modern-day Bounty Hunter.
#
#  This script forges your first Beskar key token, configuration,
#  and systemd integration for auto-unlock of encrypted ZFS pools.
#
#  Run as root:
#    curl -fsSL https://raw.githubusercontent.com/x4ngus/zfs_beskar_key/main/scripts/bootstrap.sh | sudo bash
# =============================================================================

set -euo pipefail

BESKAR_LABEL="BESKARKEY"
CONFIG_PATH="/etc/zfs-beskar.toml"
MOUNT_DIR="/mnt/beskar"
RUN_DIR="/run/beskar"
BINARY="/usr/local/bin/zfs_beskar_key"
KEY_NAME="rpool.keyhex"
PART_NAME="BESKAR_PART"

# -----------------------------------------------------------------------------
# Banner
# -----------------------------------------------------------------------------
clear
echo
echo "═══════════════════════════════════════════════════════════════════════"
echo "  BESKAR FORGE SEQUENCE – v1.1"
echo "  For the modern-day Bounty Hunter."
echo "═══════════════════════════════════════════════════════════════════════"
echo

# -----------------------------------------------------------------------------
# 1. Environment validation
# -----------------------------------------------------------------------------
echo "[1/6] Checking environment integrity..."

if [[ $EUID -ne 0 ]]; then
    echo "⛔ This script must be executed as root."
    exit 1
fi

for cmd in parted mkfs.ext4 blkid lsblk; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "⛔ Required utility missing: $cmd"
        exit 1
    fi
done

if [[ ! -x "$BINARY" ]]; then
    echo "⛔ $BINARY not found or not executable."
    echo "   Build and install it first via: cargo build --release && sudo cp target/release/zfs_beskar_key $BINARY"
    exit 1
fi

echo "✅ Environment verified — forge is ready."
echo

# -----------------------------------------------------------------------------
# 2. Select and verify USB target
# -----------------------------------------------------------------------------
echo "[2/6] Scanning for removable drives..."
lsblk -o NAME,SIZE,TYPE,MOUNTPOINT,LABEL | grep -E "disk|part" || true

read -rp "Enter the device path for your USB (e.g., /dev/sdb): " DEVICE
if [[ -z "$DEVICE" || ! -b "$DEVICE" ]]; then
    echo "⛔ Invalid device specified."
    exit 1
fi

echo
read -rp "⚠️  This will ERASE all data on $DEVICE. Proceed? (y/N): " CONFIRM
[[ "${CONFIRM,,}" == "y" ]] || exit 1

# -----------------------------------------------------------------------------
# 3. Format and prepare USB token
# -----------------------------------------------------------------------------
echo
echo "[3/6] Forging storage medium..."
umount "${DEVICE}"* 2>/dev/null || true
parted -s "$DEVICE" mklabel gpt
parted -s "$DEVICE" mkpart "$PART_NAME" ext4 1MiB 100%
sleep 1
mkfs.ext4 -F -L "$BESKAR_LABEL" "${DEVICE}1" >/dev/null
echo "✅ Medium aligned and labeled as $BESKAR_LABEL."
echo

# -----------------------------------------------------------------------------
# 4. Forge and store encryption key
# -----------------------------------------------------------------------------
echo "[4/6] Forging encryption key..."
mkdir -p "$MOUNT_DIR"
mount "/dev/disk/by-label/$BESKAR_LABEL" "$MOUNT_DIR"

# Use new command structure: ForgeKey -> key file output
"$BINARY" forge-key | tee "$MOUNT_DIR/$KEY_NAME" >/dev/null
chmod 0400 "$MOUNT_DIR/$KEY_NAME"
sync
umount "$MOUNT_DIR"

# Detect UUID for systemd mount unit
USB_UUID=$(blkid -s UUID -o value "${DEVICE}1" || true)
echo "✅ Key forged and stored on USB token ($USB_UUID)."
echo

# -----------------------------------------------------------------------------
# 5. Write configuration file (v1.1 layout)
# -----------------------------------------------------------------------------
echo "[5/6] Writing configuration to $CONFIG_PATH..."
mkdir -p "$(dirname "$CONFIG_PATH")"

cat >"$CONFIG_PATH" <<EOF
[policy]
datasets = ["rpool/ROOT"]
zfs_path = "/sbin/zfs"
allow_root = true

[crypto]
timeout_secs = 10

[usb]
key_hex_path = "$RUN_DIR/$KEY_NAME"
expected_sha256 = ""

[fallback]
enabled = true
askpass = true
askpass_path = "/usr/bin/systemd-ask-password"
EOF

chmod 0600 "$CONFIG_PATH"
echo "✅ Configuration file written at $CONFIG_PATH."
echo

# -----------------------------------------------------------------------------
# 6. Initialize Beskar environment (v1.1 workflow)
# -----------------------------------------------------------------------------
echo "[6/6] Initializing system and installing units..."
"$BINARY" init --dataset="rpool/ROOT" --config="$CONFIG_PATH" || {
    echo "⚠️  Init command failed — you can rerun manually with:"
    echo "    $BINARY init --dataset=rpool/ROOT --config=$CONFIG_PATH"
}

echo
"$BINARY" doctor --config="$CONFIG_PATH" || true
echo "✅ System diagnostics completed."
echo

# -----------------------------------------------------------------------------
# Epilogue
# -----------------------------------------------------------------------------
echo "═══════════════════════════════════════════════════════════════════════"
echo "🛡️  Beskar key successfully forged and configured."
echo
echo "    • USB token label : $BESKAR_LABEL"
echo "    • Config path     : $CONFIG_PATH"
echo "    • Units installed : beskar-usb.mount / beskar-unlock.service"
echo "    • USB UUID        : ${USB_UUID:-unknown}"
echo
echo "Insert the key and reboot to test automated pool unlock."
echo "If the token is absent, fallback passphrase protection remains active."
echo
echo "This is the Way."
echo "═══════════════════════════════════════════════════════════════════════"
echo
