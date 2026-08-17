#!/usr/bin/env bash
# Repin the abgen release that every dcl-one-sdk binary embeds.
#
#   scripts/pin-abgen.sh            # latest release
#   scripts/pin-abgen.sh v0.17.0    # a specific tag
#
# Rewrites crates/dcl-one-sdk/abgen-release.lock from the release's own
# SHA256SUMS.txt. build.rs and export-overlay/flake.nix both read that file, so
# this is the only place a version lives.
set -euo pipefail

repo=decentraland/abgen
here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock=$here/abgen-release.lock

command -v gh >/dev/null || { echo "pin-abgen needs the gh CLI" >&2; exit 1; }

tag=${1:-$(gh release view -R "$repo" --json tagName -q .tagName)}
echo "pinning abgen $tag"

sums=$(mktemp)
trap 'rm -f "$sums"' EXIT
gh release download "$tag" -R "$repo" -p SHA256SUMS.txt -O "$sums" --clobber

# Only the plain `abgen-<tag>-<target>.tar.gz` archives: the `abgen-native-`
# ones are the C ABI shared library, not the server this embeds.
targets=(
  aarch64-apple-darwin
  x86_64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-unknown-linux-gnu
  aarch64-pc-windows-gnullvm
  x86_64-pc-windows-gnu
)

body=""
for t in "${targets[@]}"; do
  name="abgen-$tag-$t.tar.gz"
  sha=$(awk -v n="$name" '$2 == n { print $1 }' "$sums")
  [ -n "$sha" ] || { echo "no $name in $tag's SHA256SUMS.txt" >&2; exit 1; }
  body+=$(printf '%-26s = %s\n' "$t" "$sha")$'\n'
done

# Keep the header comment; replace version and the target table. Note the
# trailing \n: $(...) strips it, and without it the last header line fuses onto
# the first line of the block below.
header=$(sed -n '1,/^# Keys are abgen/p' "$lock" | sed '$d')
{
  printf '%s\n' "$header"
  cat <<EOF
# Keys are abgen's release targets, not rust target triples: the bundle ships
# its own loader, so the host ABI it was linked against is irrelevant and
# *-pc-windows-msvc builds ship the -gnu archive.

version = $tag
url = https://github.com/$repo/releases/download/{version}/abgen-{version}-{target}.tar.gz

EOF
  printf '%s' "$body"
} > "$lock.new"
mv "$lock.new" "$lock"

echo "wrote $lock"
echo "next: cargo build -p dcl-one-sdk   (downloads and re-embeds)"
