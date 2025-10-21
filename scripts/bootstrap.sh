#!/usr/bin/env bash
# Mythosaur-forged bootstrapper for zfs_beskar_key.
# Formats a USB token, forges the key, writes config, and installs systemd units.

set -euo pipefail

readonly BESKAR_LABEL="${BESKAR_LABEL:-BESKARKEY}"
readonly CONFIG_PATH="${CONFIG_PATH:-/etc/zfs-beskar.toml}"
readonly MOUNT_DIR="${MOUNT_DIR:-/mnt/beskar}"
readonly RUN_DIR="${RUN_DIR:-/run/beskar}"
readonly BINARY="${BINARY:-/usr/local/bin/zfs_beskar_key}"
LEAVE_RUN_MOUNT=0
version_output=$("$BINARY" --version 2>/dev/null | awk 'NR==1 {print $NF}' || true)
readonly APP_VERSION="${APP_VERSION:-${version_output:-unknown}}"
readonly OPERATOR="${OPERATOR:-${USER:-$(id -un 2>/dev/null || echo "unknown")}}"

if command -v tput >/dev/null 2>&1; then
    RESET=$(tput sgr0)
else
    RESET=$'\e[0m'
fi
MUTED=$'\e[38;5;246m'
ACCENT=$'\e[38;5;208m'
SUCCESS=$'\e[38;5;221m'
WARN=$'\e[38;5;214m'
ERROR=$'\e[38;5;196m'

forge_banner() {
    local body_width=100
    local span=$(printf '%*s' $((body_width + 2)) '' | tr ' ' '═')
    local crest=(
"⠀⠀⠀⠀⠀⠀⠀⠀⢀⣤⣶⡄⢠⣶⣶⣶⣶⣶⣶⣾⡆⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⢿⣿⣿⣄⠙⣿⣿⣿⣿⣿⣿⣿⠇⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⡈⢿⣿⣿⣴⣿⣿⣿⣿⡿⠿⠋⣰⡇⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⣼⡇⠀⠈⠙⢿⣿⣿⡿⠋⠀⠀⢀⣿⣧⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠰⣿⣧⡀⠀⠀⠸⣿⣯⠀⠀⢀⣠⣾⣿⡿⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⢿⣿⣿⣷⣤⡀⢻⣿⢠⣾⣿⣿⣿⠋⢀⣶⣦⡀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⢀⣤⣄⡈⠻⢿⣿⣿⣧⣼⣿⣾⣿⣿⣿⠏⢀⣿⣿⣿⣿⣦⡀⠀⠀⠀"
"⠀⠀⠀⣴⣿⣿⣿⣿⣷⡄⠈⣿⣿⣿⣿⣿⣿⡟⠉⠀⠘⢿⣿⣿⣿⣿⣿⣄⠀⠀"
"⠀⢀⣾⣿⣿⣿⣿⠿⠋⠀⠀⣿⡿⣿⣿⣿⢿⣷⠀⠀⠀⠀⠙⢿⣿⣿⣿⣿⣆⠀"
"⢀⣾⣿⣿⣿⠟⠁⠀⠀⠀⠀⢸⡇⠈⣿⠁⢸⡿⠀⠀⠀⠀⠀⠀⠙⢿⣿⣿⣿⡆"
"⢸⣿⣿⡿⠁⠀⠀⠀⠀⠀⠀⢸⣿⡄⢻⢀⣿⠇⠀⠀⠀⠀⠀⠀⠀⠈⢿⣿⣿⣷"
"⣾⣿⣿⠁⠀⠀⠀⠀⠀⠀⠀⢸⣿⣧⠈⢸⣿⡀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⣿⡟"
"⢹⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠈⠻⣿⡀⢸⣿⡇⠀⠀⠀⠀⠀⠀⠀⢀⣾⣿⡿⠃"
"⠘⢿⣿⣷⣄⣀⡀⠀⠀⠀⠀⢰⣦⢹⡇⢸⡏⣴⠀⠀⠀⠀⠲⠶⠾⠿⠟⠋⠀⠀"
"⠀⠈⠙⠛⠿⠿⠟⠋⠁⠀⠀⢸⡟⢸⡇⢸⡇⢹⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⢸⡇⢸⡇⢸⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⡇⢸⡇⢸⠇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣼⡇⢸⣇⠘⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣿⡇⢸⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"
"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠃⠸⠟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"
    )
    local motif=(╳ ╂ ╋ ╂)
    local border_palette=(208 214 178 202)
    local inner_width=$body_width
    local inner_width=$((body_width - 4))

    printf '%b\n' "${ACCENT}╔${span}╗${RESET}"
    local idx=0
    for line in "${crest[@]}"; do
        local raw_len=${#line}
        local pad=$(( (inner_width - raw_len) / 2 ))
        if (( pad < 0 )); then pad=0; fi
        local right_pad=$(( inner_width - raw_len - pad ))

        local palette=(208 214 220 178 142 196 202 208)
        local palette_len=${#palette[@]}
        local tinted=""
        local char_idx=0
        local IFS=""
        for ((char_idx2=0; char_idx2 < ${#line}; char_idx2++)); do
            local ch=${line:char_idx2:1}
            if [[ $ch =~ [[:space:]] ]]; then
                tinted+="$ch"
            else
                local color=${palette[$((char_idx % palette_len))]}
                tinted+=$"\e[38;5;${color}m$ch\e[0m"
                ((char_idx++))
            fi
        done

        printf -v body '%*s%s%*s' "$pad" '' "$tinted" "$right_pad" ''

        local left_color=${border_palette[$((idx % ${#border_palette[@]}))]}
        local right_color=${border_palette[$(((idx + 1) % ${#border_palette[@]}))]}
        local motif_left=${motif[$((idx % ${#motif[@]}))]}
        local motif_right=${motif[$(((idx + 2) % ${#motif[@]}))]}

        printf -v left_edge '\e[38;5;%sm║\e[0m' "$left_color"
        printf -v right_edge '\e[38;5;%sm║\e[0m' "$right_color"
        printf -v motif_left_t '\e[38;5;%sm%s\e[0m' "$left_color" "$motif_left"
        printf -v motif_right_t '\e[38;5;%sm%s\e[0m' "$right_color" "$motif_right"
        printf '%b\n' "$left_edge$motif_left_t$body$motif_right_t$right_edge"
        ((idx++))
    done
    printf '%b\n' "${ACCENT}╚${span}╝${RESET}"
}

info() { printf '%b\n' "${ACCENT}[INFO]${RESET} $1"; }
success() { printf '%b\n' "${SUCCESS}[OK]${RESET} $1"; }
warn() { printf '%b\n' "${WARN}[WARN]${RESET} $1"; }
die() {
    printf '%b\n' "${ERROR}[ERROR]${RESET} $*" >&2
    exit 1
}

cleanup() {
    if mountpoint -q "$MOUNT_DIR"; then
        umount "$MOUNT_DIR" || true
    fi
    if [[ ${LEAVE_RUN_MOUNT:-0} -eq 0 ]] && mountpoint -q "$RUN_DIR"; then
        umount "$RUN_DIR" || true
    fi
}
trap cleanup EXIT

require_cmd() {
    local cmd=$1
    command -v "$cmd" >/dev/null 2>&1 || die "Required command not found: $cmd"
}

forge_banner
info "Beskar tribute bootstrap commencing — lay your beskar on the anvil."

[[ $EUID -eq 0 ]] || die "Run this script as root."
[[ -x $BINARY ]] || die "$BINARY not found. Install zfs_beskar_key first."

for cmd in lsblk blkid parted mkfs.ext4 sha256sum udevadm wipefs; do
    require_cmd "$cmd"
done

mkdir -p "$MOUNT_DIR" "$RUN_DIR"

printf '\n'
info "Scanning the hangar for removable vessels…"
lsblk -rpo NAME,TYPE,RM,SIZE,MODEL |
    awk '$2 == "disk" { rm = ($3 == "1") ? "yes" : "no"; printf "  %s  %s  removable=%s  %s\n", $1, $4, rm, $5 }'

printf '%b' "${ACCENT}▸${RESET} Select the carrier to temper (e.g. /dev/sdb): "
read -r DEVICE
[[ -n ${DEVICE:-} && -b $DEVICE ]] || die "Invalid block device: $DEVICE"

printf '\n'
printf '%b' "${WARN}▲${RESET} This erases all data on $DEVICE. Continue? [y/N]: "
read -r confirm
[[ ${confirm,,} == "y" ]] || die "Aborted by user."

mounted_parts=$(lsblk -nrpo NAME,MOUNTPOINT "$DEVICE" | awk '$2 != "" {print $1" -> "$2}')
[[ -z ${mounted_parts:-} ]] || die "Device has mounted partitions: $mounted_parts"

info "Creating single ext4 partition labeled $BESKAR_LABEL..."
wipefs -a "$DEVICE" >/dev/null 2>&1 || true
parted -s "$DEVICE" mklabel gpt
parted -s "$DEVICE" mkpart primary ext4 1MiB 100%
udevadm settle

PARTITION=$(lsblk -prno NAME,TYPE "$DEVICE" | awk '$2=="part"{print $1; exit}')
[[ -n $PARTITION ]] || die "Failed to detect new partition on $DEVICE"

mkfs.ext4 -F -L "$BESKAR_LABEL" "$PARTITION" >/dev/null
udevadm settle

mount "$PARTITION" "$MOUNT_DIR"

printf '%b' "${ACCENT}▸${RESET} Dataset to unlock [rpool/ROOT]: "
read -r DATASET_INPUT
DATASET=${DATASET_INPUT:-rpool/ROOT}
[[ -n $DATASET ]] || die "Dataset name must not be empty."

KEY_BASENAME=$(echo "$DATASET" | tr '/[:space:]' '_' | tr '[:upper:]' '[:lower:]')
KEY_NAME="${KEY_BASENAME}.keyhex"
KEY_PATH="$MOUNT_DIR/$KEY_NAME"

info "Forging key material and inscribing checksum sigils…"
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

printf '\n'
info "Etching armory doctrine to $CONFIG_PATH…"
if [[ -f $CONFIG_PATH ]]; then
    warn "Existing creed detected at $CONFIG_PATH."
    printf '%b' "${WARN}▲${RESET} Overwrite with new settings? [y/N]: "
    read -r overwrite
    [[ ${overwrite,,} == "y" ]] || die "Keeping existing config; aborting to avoid clobbering."
fi
mkdir -p "$(dirname "$CONFIG_PATH")"
cat >"$CONFIG_PATH" <<EOF
[policy]
datasets = ["$DATASET"]
zfs_path = "$ZFS_BIN"
binary_path = "$BINARY"
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

info "Installing systemd sentries via zfs_beskar_key…"
"$BINARY" install-units --config "$CONFIG_PATH" --dataset "$DATASET"

printf '\n'
info "Mounting the Beskar token at $RUN_DIR for immediate use…"
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
success "Beskar token mounted. Key path confirmed at $RUN_DIR/$KEY_NAME."

printf '\n'
if command -v dracut >/dev/null 2>&1; then
    printf '%b' "${ACCENT}▸${RESET} Run dracut -f now to refresh initramfs with the Beskar module? [y/N]: "
    read -r rebuild
    if [[ ${rebuild,,} == "y" ]]; then
        info "Rebuilding initramfs via dracut…"
        dracut -f || die "dracut failed — inspect output above and rerun manually."
    else
        warn "Skipping initramfs rebuild. Run 'sudo dracut -f' before rebooting."
    fi
else
    warn "dracut binary not found. Install dracut and run 'sudo dracut -f' before reboot."
fi

printf '\n'
success "Bootstrap complete. This is the Way."
printf '%b\n' "${MUTED}  • USB label : $BESKAR_LABEL${RESET}"
printf '%b\n' "${MUTED}  • USB UUID  : $USB_UUID${RESET}"
printf '%b\n' "${MUTED}  • Key file  : $RUN_DIR/$KEY_NAME${RESET}"
printf '%b\n' "${MUTED}  • Config    : $CONFIG_PATH${RESET}"
printf '\n'
info "Next steps:"
printf '%b\n' "${MUTED}  - Keep the USB inserted for boot-time unlock.${RESET}"
printf '%b\n' "${MUTED}  - Run 'sudo zfs_beskar_key --menu' and choose 'Vault Drill Simulation'.${RESET}"
printf '%b\n' "${MUTED}  - Run '$BINARY self-test --config $CONFIG_PATH --dataset $DATASET'.${RESET}"
printf '\n'
success "Forge sequence concluded."
