#!/usr/bin/env bash
# Minimal bootstrapper for zfs_beskar_key.
# Formats a USB token, forges the key, writes config, and installs systemd units.

set -euo pipefail

readonly BESKAR_LABEL="${BESKAR_LABEL:-BESKARKEY}"
readonly CONFIG_PATH="${CONFIG_PATH:-/etc/zfs-beskar.toml}"
readonly MOUNT_DIR="${MOUNT_DIR:-/mnt/beskar}"
readonly RUN_DIR="${RUN_DIR:-/run/beskar}"
readonly BINARY="${BINARY:-/usr/local/bin/zfs_beskar_key}"
LEAVE_RUN_MOUNT=0

cleanup() {
    if mountpoint -q "$MOUNT_DIR"; then
        umount "$MOUNT_DIR" || true
    fi
    if [[ ${LEAVE_RUN_MOUNT:-0} -eq 0 ]] && mountpoint -q "$RUN_DIR"; then
        umount "$RUN_DIR" || true
    fi
}
trap cleanup EXIT

die() {
    echo "Error: $*" >&2
    exit 1
}

require_cmd() {
    local cmd=$1
    command -v "$cmd" >/dev/null 2>&1 || die "Required command not found: $cmd"
}

echo "Beskar bootstrap starting..."

[[ $EUID -eq 0 ]] || die "Run this script as root."
[[ -x $BINARY ]] || die "$BINARY not found. Install zfs_beskar_key first."

for cmd in lsblk blkid parted mkfs.ext4 sha256sum udevadm wipefs; do
    require_cmd "$cmd"
done

mkdir -p "$MOUNT_DIR" "$RUN_DIR"

echo
echo "Removable devices:"
lsblk -rpo NAME,TYPE,RM,SIZE,MODEL |
    awk '$2 == "disk" { rm = ($3 == "1") ? "yes" : "no"; printf "  %s  %s  removable=%s  %s\n", $1, $4, rm, $5 }'

read -rp "USB device to format (e.g. /dev/sdb): " DEVICE
[[ -n ${DEVICE:-} && -b $DEVICE ]] || die "Invalid block device: $DEVICE"

echo
read -rp "This will erase all data on $DEVICE. Continue? [y/N]: " confirm
[[ ${confirm,,} == "y" ]] || die "Aborted by user."

# Avoid accidental root disk wipe
mounted_parts=$(lsblk -nrpo NAME,MOUNTPOINT "$DEVICE" | awk '$2 != "" {print $1" -> "$2}')
[[ -z ${mounted_parts:-} ]] || die "Device has mounted partitions: $mounted_parts"

echo "Creating single ext4 partition labeled $BESKAR_LABEL..."
wipefs -a "$DEVICE" >/dev/null 2>&1 || true
parted -s "$DEVICE" mklabel gpt
parted -s "$DEVICE" mkpart primary ext4 1MiB 100%
udevadm settle

PARTITION=$(lsblk -prno NAME,TYPE "$DEVICE" | awk '$2=="part"{print $1; exit}')
[[ -n $PARTITION ]] || die "Failed to detect new partition on $DEVICE"

mkfs.ext4 -F -L "$BESKAR_LABEL" "$PARTITION" >/dev/null
udevadm settle

mount "$PARTITION" "$MOUNT_DIR"

read -rp "Dataset to unlock [rpool/ROOT]: " DATASET_INPUT
DATASET=${DATASET_INPUT:-rpool/ROOT}
[[ -n $DATASET ]] || die "Dataset name must not be empty."

KEY_BASENAME=$(echo "$DATASET" | tr '/[:space:]' '_' | tr '[:upper:]' '[:lower:]')
KEY_NAME="${KEY_BASENAME}.keyhex"
KEY_PATH="$MOUNT_DIR/$KEY_NAME"

echo "Forging key material..."
KEY_HEX=$("$BINARY" forge-key | head -n 1 | tr -d '[:space:]')
[[ ${#KEY_HEX} -eq 64 && $KEY_HEX =~ ^[0-9a-fA-F]+$ ]] || die "Unexpected forge-key output."

printf '%s\n' "$KEY_HEX" >"$KEY_PATH"
chmod 0400 "$KEY_PATH"
sync
USB_SHA=$(sha256sum "$KEY_PATH" | awk '{print $1}')

umount "$MOUNT_DIR"

USB_UUID=$(blkid -s UUID -o value "$PARTITION" || true)
[[ -n $USB_UUID ]] || die "Unable to determine USB UUID for $PARTITION"

ZFS_BIN=$(command -v zfs || echo "/sbin/zfs")

echo
echo "Writing config to $CONFIG_PATH..."
if [[ -f $CONFIG_PATH ]]; then
    echo "Existing config detected at $CONFIG_PATH."
    read -rp "Overwrite with new settings? [y/N]: " overwrite
    [[ ${overwrite,,} == "y" ]] || die "Keeping existing config; aborting to avoid clobbering."
fi
mkdir -p "$(dirname "$CONFIG_PATH")"
cat >"$CONFIG_PATH" <<EOF
[policy]
datasets = ["$DATASET"]
zfs_path = "$ZFS_BIN"
allow_root = true

[crypto]
timeout_secs = 10

[usb]
key_hex_path = "$RUN_DIR/$KEY_NAME"
expected_sha256 = "$USB_SHA"

[fallback]
enabled = true
askpass = true
askpass_path = "/usr/bin/systemd-ask-password"
EOF

chmod 600 "$CONFIG_PATH"

echo "Installing systemd units via zfs_beskar_key..."
"$BINARY" install-units --config "$CONFIG_PATH" --dataset "$DATASET"

echo
echo "Mounting the Beskar token at $RUN_DIR for immediate use..."
if mountpoint -q "$RUN_DIR"; then
    umount "$RUN_DIR" || die "Unable to release existing mount at $RUN_DIR"
fi
mount "$PARTITION" "$RUN_DIR"
udevadm settle || true
if [[ ! -f "$RUN_DIR/$KEY_NAME" ]]; then
    die "Key file $RUN_DIR/$KEY_NAME not found after mounting."
fi
chmod 0400 "$RUN_DIR/$KEY_NAME"
LEAVE_RUN_MOUNT=1
echo "Beskar token mounted. Key path confirmed at $RUN_DIR/$KEY_NAME."

echo
if command -v dracut >/dev/null 2>&1; then
    read -rp "Run dracut -f now to refresh initramfs with the Beskar module? [y/N]: " rebuild
    if [[ ${rebuild,,} == "y" ]]; then
        echo "Rebuilding initramfs via dracut..."
        dracut -f || die "dracut failed — inspect output above and rerun manually."
    else
        echo "Skipping initramfs rebuild. Run 'sudo dracut -f' before rebooting."
    fi
else
    echo "⚠️  dracut binary not found. Install dracut and run 'sudo dracut -f' before reboot."
fi

echo
echo "Bootstrap complete."
echo "  • USB label : $BESKAR_LABEL"
echo "  • USB UUID  : $USB_UUID"
echo "  • Key file  : $RUN_DIR/$KEY_NAME"
echo "  • Config    : $CONFIG_PATH"
echo
echo "Next steps:"
echo "  - Keep the USB inserted for boot-time unlock."
echo "  - Run 'sudo zfs_beskar_key --menu' and choose 'Vault Drill Simulation' to rehearse."
echo "  - Run '$BINARY self-test --config $CONFIG_PATH --dataset $DATASET' to verify the USB."
echo
echo "Finished."
