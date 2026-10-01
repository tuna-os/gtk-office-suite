#!/usr/bin/env bash
# Build the Tier C guest image once: Fedora Cloud + GNOME session + Flatpak
# runtime. CI caches the result (keyed on this script and user-data), so
# nightly runs only boot it.
#
#   tools/render-lab/vm/prepare.sh <workdir>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="${1:?workdir}"
IMAGE=Fedora-Cloud-Base-Generic-42-1.1.x86_64.qcow2
# download.fedoraproject.org redirects to a random mirror, and mirrors drop
# a release once it reaches end of life: every nightly Tier C run failed
# here on a mirror's 404 (#1199). The archive keeps every release for good,
# so it is the fallback; FEDORA_URL still overrides both.
FEDORA_URLS=(
    "${FEDORA_URL:-https://download.fedoraproject.org/pub/fedora/linux/releases/42/Cloud/x86_64/images/$IMAGE}"
    "https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/42/Cloud/x86_64/images/$IMAGE"
)
mkdir -p "$WORK"; cd "$WORK"

[ -f id_lab ] || ssh-keygen -q -t ed25519 -N '' -f id_lab
if [ ! -f base.qcow2 ]; then
    for url in "${FEDORA_URLS[@]}"; do
        curl -fL --retry 3 -o base.qcow2.part "$url" && mv base.qcow2.part base.qcow2 && break
        echo "could not fetch $url; trying the next source" >&2
    done
    [ -f base.qcow2 ] || { echo "no source served $IMAGE" >&2; exit 1; }
fi
cp base.qcow2 guest.qcow2
qemu-img resize guest.qcow2 20G

sed "s|__SSH_KEY__|$(cat id_lab.pub)|" "$HERE/user-data" > user-data
printf 'instance-id: render-lab\nlocal-hostname: render-lab\n' > meta-data
cloud-localds seed.iso user-data meta-data

# First boot runs cloud-init, which installs everything and powers off.
timeout 45m qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m 6G \
    -drive file=guest.qcow2,if=virtio -drive file=seed.iso,media=cdrom \
    -nic user,model=virtio -display none -serial file:prepare-serial.log
echo "prepared: $WORK/guest.qcow2"
