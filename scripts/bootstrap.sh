#!/usr/bin/env bash
# Mythosaur-forged bootstrapper for zfs_beskar_key.
# Hands the heavy lifting to the Rust forge so the bootstrap workflow
# mirrors the primary init logic (ZFS re-key, USB burn, config write).

set -euo pipefail

readonly BESKAR_LABEL="${BESKAR_LABEL:-BESKARKEY}"
readonly CONFIG_OVERRIDE="${CONFIG_PATH:-}"
readonly CONFIG_PATH="/etc/zfs-beskar.toml"
readonly RUN_DIR="/run/beskar"
readonly BINARY="${BINARY:-/usr/local/bin/zfs_beskar_key}"

version_output=""
if [[ -x $BINARY ]]; then
    version_output=$("$BINARY" --version 2>/dev/null | awk 'NR==1 {print $NF}' || true)
fi
readonly APP_VERSION="${APP_VERSION:-${version_output:-unknown}}"
readonly OPERATOR="${OPERATOR:-${USER:-$(id -un 2>/dev/null || echo "unknown")}}"

mkdir -p "$RUN_DIR"

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
    local span
    span=$(printf '%*s' $((body_width + 2)) '' | tr ' ' '═')
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
    local inner_width=$((body_width - 4))

    printf '%b\n' "${ACCENT}╔${span}╗${RESET}"
    local idx=0
    for line in "${crest[@]}"; do
        local raw_len=${#line}
        local pad=$(((inner_width - raw_len) / 2))
        if (( pad < 0 )); then pad=0; fi
        local right_pad=$((inner_width - raw_len - pad))

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

verify_initramfs_artifacts() {
    local dataset="$1"
    local kernel="${2:-$(uname -r)}"
    local image="/boot/initrd.img-${kernel}"

    if ! command -v lsinitrd >/dev/null 2>&1; then
        warn "lsinitrd not available; skipping initramfs verification (dracut module already stamped)."
        return
    fi
    if [[ ! -f $image ]]; then
        warn "Initramfs image $image not found; skipping verification."
        return
    fi

    local manifest
    if ! manifest=$(lsinitrd "$image" 2>/dev/null); then
        warn "Unable to inspect $image via lsinitrd; run the command manually."
        return
    fi

    local -a missing=()
    grep -q 'beskar-load-key' <<<"$manifest" || missing+=("beskar-load-key.service")
    grep -q 'zfs-load-key.service.d/beskar.conf' <<<"$manifest" || missing+=("zfs-load-key drop-in")
    grep -q 'zfs-load-module.service.d/beskar.conf' <<<"$manifest" || missing+=("zfs-load-module drop-in")

    if ((${#missing[@]} == 0)); then
        success "Initramfs ${image} contains the Beskar loader and systemd drop-ins."
    else
        warn "Initramfs ${image} is missing: ${missing[*]}. Run '$BINARY --config $CONFIG_PATH install-dracut --dataset ${dataset}' after resolving."
    fi
}

info() { printf '%b\n' "${ACCENT}[INFO]${RESET} $1"; }
success() { printf '%b\n' "${SUCCESS}[OK]${RESET} $1"; }
warn() { printf '%b\n' "${WARN}[WARN]${RESET} $1"; }
die() {
    printf '%b\n' "${ERROR}[ERROR]${RESET} $*" >&2
    exit 1
}

require_cmd() {
    local cmd=$1
    command -v "$cmd" >/dev/null 2>&1 || die "Required command not found: $cmd"
}

forge_banner
info "Armorer console online. Tribute inbound."

if [[ -n $CONFIG_OVERRIDE && $CONFIG_OVERRIDE != "$CONFIG_PATH" ]]; then
    warn "CONFIG_PATH override (${CONFIG_OVERRIDE}) ignored; creed lives at ${CONFIG_PATH}."
fi

[[ $EUID -eq 0 ]] || die "Run this script as root."
[[ -x $BINARY ]] || die "$BINARY missing. Install zfs_beskar_key before invoking the forge."

mkdir -p "$RUN_DIR"

for cmd in lsblk blkid parted mkfs.ext4 udevadm sha256sum zfs; do
    require_cmd "$cmd"
done

printf '\n'
info "Sweeping hangar for candidate vessels."
lsblk -rpo NAME,TYPE,RM,SIZE,MODEL |
    awk '$2 == "disk" { rm = ($3 == "1") ? "yes" : "no"; printf "  %s  %s  removable=%s  %s\n", $1, $4, rm, $5 }'

printf '%b' "${ACCENT}▸${RESET} Select target device (e.g. /dev/sdb): "
read -r DEVICE
[[ -n ${DEVICE:-} && -b $DEVICE ]] || die "Not a valid vessel: $DEVICE"

printf '\n'
printf '%b' "${WARN}▲${RESET} This purge is irreversible. Continue? [y/N]: "
read -r confirm
[[ ${confirm,,} == "y" ]] || die "Aborted by user."

mounted_parts=$(lsblk -nrpo NAME,MOUNTPOINT "$DEVICE" | awk '$2 != "" {print $1" -> "$2}')
[[ -z ${mounted_parts:-} ]] || die "Device has mounted partitions: $mounted_parts"

printf '\n'
DEFAULT_DATASET=$(zfs list -H -o name,mountpoint -t filesystem 2>/dev/null | awk -F '\t' '$2 == "/" {print $1; exit}')
if [[ -z $DEFAULT_DATASET ]]; then
    DEFAULT_DATASET="rpool/ROOT"
else
    info "Detected $DEFAULT_DATASET guarding root. Target locked."
fi

printf '%b' "${ACCENT}▸${RESET} Dataset to unlock [$DEFAULT_DATASET]: "
read -r DATASET_INPUT
DATASET=${DATASET_INPUT:-$DEFAULT_DATASET}
[[ -n $DATASET ]] || die "Dataset name must not be empty."

printf '%b' "${ACCENT}▸${RESET} Invoke safe mode (manual confirmations, no forced wipe)? [y/N]: "
read -r safe_answer
SAFE_FLAG=()
if [[ ${safe_answer,,} == "y" ]]; then
    SAFE_FLAG=(--safe)
fi

printf '\n'
info "Summoning zfs_beskar_key v${APP_VERSION} to temper."
INIT_CMD=(
    "$BINARY"
    "--config" "$CONFIG_PATH"
    "-d" "$DATASET"
    init
    "--usb-device" "$DEVICE"
    "${SAFE_FLAG[@]}"
)

if ! "${INIT_CMD[@]}"; then
    die "Forge command failed; inspect the output above."
fi

ENCRYPTION_ROOT=$(zfs get -H -o value encryptionroot "$DATASET" 2>/dev/null | awk 'NR==1 {print $1}' || true)
if [[ -z ${ENCRYPTION_ROOT:-} || ${ENCRYPTION_ROOT} == "-" ]]; then
    ENCRYPTION_ROOT="$DATASET"
fi
KEY_LOCATION=$(zfs get -H -o value keylocation "$ENCRYPTION_ROOT" 2>/dev/null | awk 'NR==1 {print $1}' || true)
if [[ -n ${KEY_LOCATION:-} ]]; then
    info "Encryption root $ENCRYPTION_ROOT now advertises $KEY_LOCATION."
fi
success "Init pass complete; dracut carries the Beskar loader."

printf '\n'
info "Posting systemd sentries at their watch."
INSTALL_CMD=(
    "$BINARY"
    "--config" "$CONFIG_PATH"
    install-units
)

if ! "${INSTALL_CMD[@]}"; then
    die "Systemd unit installation failed. Run '$BINARY install-units' manually once resolved."
fi
verify_initramfs_artifacts "$DATASET"

printf '\n'
KEY_PATH=$(grep -E '^key_hex_path' "$CONFIG_PATH" 2>/dev/null | head -n1 | cut -d'"' -f2 || true)
USB_SHA=$(grep -E '^expected_sha256' "$CONFIG_PATH" 2>/dev/null | head -n1 | cut -d'"' -f2 || true)
USB_UUID=$(blkid | grep "$BESKAR_LABEL" | sed -n 's/.*UUID=\"\([^\"]*\)\".*/\1/p' | head -n1 || true)

if mountpoint -q "$RUN_DIR"; then
    success "Beskar token mounted at $RUN_DIR."
else
    warn "Token not mounted at $RUN_DIR; check manually."
fi

printf '%b\n' "${MUTED}  • USB label : $BESKAR_LABEL${RESET}"
if [[ -n $USB_UUID ]]; then
    printf '%b\n' "${MUTED}  • USB UUID  : $USB_UUID${RESET}"
else
    printf '%b\n' "${WARN}[WARN]${RESET} USB UUID missing; ensure /dev/disk/by-label/$BESKAR_LABEL exists."
fi
if [[ -n $KEY_PATH ]]; then
    printf '%b\n' "${MUTED}  • Key file  : $KEY_PATH${RESET}"
else
    printf '%b\n' "${WARN}[WARN]${RESET} key_hex_path missing from $CONFIG_PATH — inspect config."
fi
printf '%b\n' "${MUTED}  • Config    : $CONFIG_PATH${RESET}"
if [[ -n $USB_SHA ]]; then
    printf '%b\n' "${MUTED}  • Key SHA   : ${USB_SHA:0:16}…${RESET}"
fi

printf '\n'
info "Next steps:"
printf '%b\n' "${MUTED}  - Run 'sudo $BINARY doctor --config $CONFIG_PATH --dataset $DATASET'.${RESET}"
printf '%b\n' "${MUTED}  - Keep the USB inserted for boot-time unlock routines.${RESET}"
printf '%b\n' "${MUTED}  - Reboot once comfortable; the unlock sentries stand ready.${RESET}"

printf '\n'
success "Bootstrap complete. This is the Way."
