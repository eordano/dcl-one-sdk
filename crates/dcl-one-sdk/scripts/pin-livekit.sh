#!/usr/bin/env bash
# Repin livekit-release.lock to a livekit-server release.
#   scripts/pin-livekit.sh [vX.Y.Z]     default: the latest release
set -euo pipefail

repo=livekit/livekit
here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock=$here/livekit-release.lock

for tool in curl awk; do
  command -v "$tool" >/dev/null || { echo "pin-livekit needs $tool on PATH" >&2; exit 1; }
done

tag=${1:-}
if [ -z "$tag" ]; then
  latest=$(curl -fsSIL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
  tag=${latest##*/releases/tag/}
fi
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "'$tag' is not a livekit release tag (vX.Y.Z)" >&2; exit 1; }
semver=${tag#v}
echo "pinning livekit-server $tag"

sums=$(mktemp)
trap 'rm -f "$sums"' EXIT
curl -fsSL -o "$sums" "https://github.com/$repo/releases/download/$tag/checksums.txt" \
  || { echo "no checksums.txt for $tag under https://github.com/$repo/releases" >&2; exit 1; }

assets=(
  linux_amd64.tar.gz
  linux_arm64.tar.gz
  linux_armv7.tar.gz
  windows_amd64.zip
  windows_arm64.zip
)

body=""
for a in "${assets[@]}"; do
  name="livekit_${semver}_$a"
  sha=$(awk -v n="$name" '$2 == n { print $1 }' "$sums")
  [ -n "$sha" ] || { echo "no $name in $tag's checksums.txt" >&2; exit 1; }
  body+=$(printf '%-20s = %s\n' "$a" "$sha")$'\n'
done

grep -q '^# Keys are the release asset' "$lock" || { echo "$lock lost its '# Keys are the release asset' header line" >&2; exit 1; }
header=$(sed -n '1,/^# Keys are the release asset/p' "$lock" | sed '$d')
{
  printf '%s\n' "$header"
  cat <<LOCK
# Keys are the release asset suffixes (os_arch.ext), hashes from the release's
# checksums.txt.

version = $tag
url = https://github.com/$repo/releases/download/{version}/livekit_{semver}_{asset}

LOCK
  printf '%s' "$body"
} > "$lock.new"
if cmp -s "$lock.new" "$lock"; then
  rm -f "$lock.new"
  echo "unchanged $lock"
else
  mv "$lock.new" "$lock"
  echo "wrote $lock"
fi
echo "next: cargo build -p dcl-one-sdk   (downloads and re-embeds)"
