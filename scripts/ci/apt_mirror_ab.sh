#!/usr/bin/env bash
# One hosted runner's mirror sample. Never install anything or weaken APT trust.
set -euo pipefail
export LC_ALL=C

priority=${1:?supply azure-first or canonical-first}
evidence=${2:?supply an evidence directory}
case "$priority" in azure-first|canonical-first) ;; *) exit 2 ;; esac
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_OS:-} == Linux ]]
mkdir -p "$evidence"

# Match Linux WebView's local-desktop step from ce925e88 exactly.
packages=(libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
  patchelf libmpv-dev ffmpeg xvfb dbus-x11 python3-gi gir1.2-gtk-3.0 libxtst6
  openbox x11-utils iproute2 openssl)
printf '%s\n' "${packages[@]}" > "$evidence/requested-packages.txt"
printf '%s\n' "${ImageOS:-unknown} ${ImageVersion:-unknown}" | tee "$evidence/image.txt"
apt-config dump Acquire::Retries Acquire::http::Timeout Acquire::https::Timeout \
  | tee "$evidence/acquire-config.txt"

mirror_file=/etc/apt/apt-mirrors.txt
[[ -f $mirror_file && ! -L $mirror_file ]]
cp -p -- "$mirror_file" "$evidence/mirrors-original.txt"
# Both arms verify the exact configured list. The experimental arm changes
# only the preference of two existing URIs, never the source stanzas/keyrings.
python3 - "$evidence" "$priority" <<'PY'
import sys
from pathlib import Path

evidence, priority = Path(sys.argv[1]), sys.argv[2]
original = (evidence / 'mirrors-original.txt').read_bytes()
expected = (
    b'http://azure.archive.ubuntu.com/ubuntu/\tpriority:1\n'
    b'https://archive.ubuntu.com/ubuntu/\tpriority:2\n'
    b'https://security.ubuntu.com/ubuntu/\tpriority:3\n'
)
if original != expected:
    raise SystemExit('Unexpected runner mirror configuration; refusing to change the source')
if priority == 'canonical-first':
    original = original.replace(b'priority:1', b'priority:2', 1).replace(
        b'https://archive.ubuntu.com/ubuntu/\tpriority:2',
        b'https://archive.ubuntu.com/ubuntu/\tpriority:1',
        1,
    )
(evidence / 'mirrors-effective.txt').write_bytes(original)
PY

scratch=$(mktemp -d /tmp/cumulus-apt-mirror-ab.XXXXXXXXXX)
changed=false
cleanup() {
  local result=$?
  if [[ $changed == true ]]; then
    sudo cp -p -- "$scratch/mirrors-backup.txt" "$mirror_file" || result=1
  fi
  sudo rm -rf -- "${scratch:?}" || result=1
  exit "$result"
}
trap cleanup EXIT
chmod 0755 "$scratch"
mkdir -m 0755 "$scratch/archives"
if [[ $priority == canonical-first ]]; then
  sudo cp -p -- "$mirror_file" "$scratch/mirrors-backup.txt"
  changed=true
  sudo cp -- "$evidence/mirrors-effective.txt" "$mirror_file"
fi
cmp -- "$mirror_file" "$evidence/mirrors-effective.txt"
cat "$evidence/mirrors-effective.txt"
apt_flags=(-o "Dir::Cache::archives=$scratch/archives")
printf 'stage\telapsed_ms\texit_code\n' > "$evidence/timing.tsv"

timed() {
  local stage=$1 start finish status=0
  shift
  start=$(date +%s%3N)
  "$@" 2>&1 | tee "$evidence/$stage.log" || status=$?
  finish=$(date +%s%3N)
  printf '%s\t%s\t%s\n' "$stage" "$((finish - start))" "$status" \
    | tee -a "$evidence/timing.tsv"
  return "$status"
}

timed update sudo apt-get "${apt_flags[@]}" -o APT::Update::Error-Mode=any update
sudo apt-get "${apt_flags[@]}" -s install -y "${packages[@]}" | tee "$evidence/plan.log"
grep '^Inst ' "$evidence/plan.log" | sort > "$evidence/plan-installs.txt"
if grep -q '^Remv ' "$evidence/plan.log"; then
  echo 'Unexpected package removal in simulation' >&2
  exit 1
fi
timed download sudo apt-get "${apt_flags[@]}" --download-only install -y "${packages[@]}"
(
  cd "$scratch/archives"
  sha256sum -- ./*.deb
) > "$evidence/archives.sha256"
{
  printf '\n| Stage | Milliseconds | Exit |\n|---|---:|---:|\n'
  awk -F '\t' 'NR>1 { printf "| %s | %s | %s |\n", $1, $2, $3 }' "$evidence/timing.tsv"
  printf '\nMirror priority: %s. Downloaded archives: %s.\n' \
    "$priority" "$(wc -l < "$evidence/archives.sha256")"
} >> "$GITHUB_STEP_SUMMARY"
