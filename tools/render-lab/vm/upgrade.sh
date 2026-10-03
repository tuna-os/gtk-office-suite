#!/usr/bin/env bash
# Upgrading from the prior release (#1209), inside the Tier C guest's GNOME
# session, after run.sh's other checks:
#
#   upgrade.sh <app> <prior.flatpak> <candidate.flatpak> <document> <action> [<prior-name> <candidate-name>]
#   upgrade.sh --close <app>
#
# Installs the prior release on a clean slate and uses it the way a user
# does: changes two settings, opens <document>, edits it (<action>, an app
# action that changes the document without a dialog), lets the autosave
# write its recovery checkpoint, and then the app dies mid-edit. Then it
# installs the candidate over it, as a user installing the new bundle does,
# and checks that the settings are still set and that the candidate, on
# its first launch, takes the prior release's checkpoint back. Prints one
# JSON line, which upgrade_check.py judges.
#
# The candidate stays open, so run.sh can screenshot it ("(Recovered)" in
# its title), and is closed with `upgrade.sh --close <app>`.
set -u
export XDG_RUNTIME_DIR="/run/user/$(id -u)" WAYLAND_DISPLAY=wayland-0
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
if [ "${1:-}" = --close ]; then
    flatpak kill "org.tunaos.$2" 2>/dev/null || true
    exit 0
fi
app="$1"; prior="$2"; candidate="$3"; doc="$4"; action="$5"
prior_name="${6:-}"; candidate_name="${7:-}"
id="org.tunaos.$app"
state="$HOME/.var/app/$id/.local/state/$app"
gs() { flatpak run --command=gsettings "$id" "$@" 2>/dev/null; }
# The app is up once it answers on the session bus.
wait_up() {
    for _ in $(seq 60); do
        gapplication list-actions "$id" >/dev/null 2>&1 && return 0
        sleep 1
    done
    return 1
}
# Each checkpoint file with its checksum, and the legacy metadata files
# beside them. The prior release writes a checkpoint as a pair (data and
# .snapshot.meta); the candidate, adopting it, rewrites it as one envelope
# and removes the pair. Names can't tell the two apart: the sandbox's pid
# namespace gives both runs the same pid, which the ids are made from.
snapshots() { (cd "$state" 2>/dev/null && md5sum -- *.snapshot 2>/dev/null | awk '{print $2 "=" $1}') | sort; }
metas() { (cd "$state" 2>/dev/null && ls -1 -- *.snapshot.meta 2>/dev/null) | sort; }

# The prior release, from nothing: no settings and no checkpoints left by
# the checks before this one.
flatpak kill "$id" 2>/dev/null || true
flatpak uninstall --user -y --noninteractive --delete-data "$id" >/dev/null 2>&1 || true
flatpak install --user -y --noninteractive "$prior" >/dev/null 2>&1
prior_version="${prior_name:-$(flatpak info --user "$id" 2>/dev/null | sed -n 's/^ *Version: //p')}"

gs set "$id" dark-mode true
gs set "$id" window-width 1111

setsid flatpak run --filesystem=home "$id" "$doc" </dev/null >"/tmp/upgrade-$app-prior.log" 2>&1 &
prior_up=false
wait_up && prior_up=true
sleep 3
gapplication action "$id" "$action" >>"/tmp/upgrade-$app-prior.log" 2>&1
sleep 1
gapplication action "$id" autosave-now >>"/tmp/upgrade-$app-prior.log" 2>&1
sleep 2
written="$(snapshots)"
written_metas="$(metas)"
# Killed, not closed: the checkpoint is left as a crash leaves it.
flatpak kill "$id" 2>/dev/null || true
sleep 2

# The candidate over it. A bundle has no remote to update from, so a user
# installs the new bundle in its place; that keeps ~/.var/app.
flatpak install --user -y --noninteractive --reinstall "$candidate" >/dev/null 2>&1
candidate_version="${candidate_name:-$(flatpak info --user "$id" 2>/dev/null | sed -n 's/^ *Version: //p')}"

# Read before the candidate runs: closing a window writes its size.
dark="$(gs get "$id" dark-mode)"
width="$(gs get "$id" window-width)"

setsid flatpak run "$id" </dev/null >"/tmp/upgrade-$app-candidate.log" 2>&1 &
candidate_up=false
wait_up && candidate_up=true
sleep 5
after="$(snapshots)"
after_metas="$(metas)"
running=false
flatpak ps --columns=application 2>/dev/null | grep -qx "$id" && running=true

python3 - "$app" "$prior_version" "$candidate_version" "$prior_up" "$candidate_up" "$dark" "$width" \
    "$written" "$after" "$written_metas" "$after_metas" "$running" <<'PY'
import json, sys
(app, prior_v, cand_v, prior_up, cand_up, dark, width,
 written, after, written_metas, after_metas, running) = sys.argv[1:]
lines = lambda text: [s for s in text.split("\n") if s]
written, after = lines(written), lines(after)
written_metas, after_metas = lines(written_metas), lines(after_metas)
print(json.dumps({
    "app": app,
    "prior_version": prior_v,
    "candidate_version": cand_v,
    "prior_started": prior_up == "true",
    "candidate_started": cand_up == "true",
    "settings_kept": dark.strip() == "true" and width.strip() == "1111",
    "settings": {"dark-mode": dark.strip(), "window-width": width.strip()},
    "checkpoint_written": bool(written),
    # Taken back: the prior release's checkpoint is gone, rewritten as one
    # of the candidate's own (AutosaveSlot::adopt_recovered), and no legacy
    # metadata is left.
    "checkpoint_recovered": bool(written) and bool(after) and not set(written) & set(after) and not after_metas,
    "snapshots": {"written": written, "written_metas": written_metas, "after": after, "after_metas": after_metas},
    "running": running == "true",
}))
PY
