#!/usr/bin/env bash
set -euo pipefail

repo=decentraland/abgen
here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock=$here/abgen-release.lock
one=$(cd "$here/../../.." && pwd)
gate=""
for g in "$one"/*/scripts/check-abgen-pins.sh; do
  if [ -f "$g" ]; then gate=$g; break; fi
done
mirror=${ABGEN_MIRROR:-$HOME/github.com-decentraland/abgen}

for tool in curl jq git; do
  command -v "$tool" >/dev/null || { echo "pin-abgen needs $tool on PATH" >&2; exit 1; }
done

tag=${1:-}
if [ -z "$tag" ]; then
  latest=$(curl -fsSIL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
  tag=${latest##*/releases/tag/}
fi
[[ "$tag" =~ ^v[0-9][A-Za-z0-9.+-]*$ ]] || { echo "'$tag' is not an abgen release tag (vX.Y.Z)" >&2; exit 1; }
echo "pinning abgen $tag"

sums=$(mktemp)
trap 'rm -f "$sums"' EXIT
curl -fsSL -o "$sums" "https://github.com/$repo/releases/download/$tag/SHA256SUMS.txt" \
  || { echo "no SHA256SUMS.txt for $tag under https://github.com/$repo/releases" >&2; exit 1; }

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

grep -q '^# Keys are abgen' "$lock" || { echo "$lock lost its '# Keys are abgen' header line" >&2; exit 1; }
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
if cmp -s "$lock.new" "$lock"; then
  rm -f "$lock.new"
  echo "unchanged $lock"
else
  mv "$lock.new" "$lock"
  echo "wrote $lock"
fi

if [ ! -f "$gate" ] || [ ! -f "$one/catalyrst/flake.nix" ]; then
  echo "standalone tree: no flake declares abgen here"
  echo "next: cargo build -p dcl-one-sdk   (downloads and re-embeds)"
  exit 0
fi

resolve_rev() {
  local sha
  if [ -d "$mirror" ] && sha=$(git -C "$mirror" rev-parse --verify --quiet "refs/tags/$tag^{commit}" 2>/dev/null); then
    echo "$sha"
    return 0
  fi
  if command -v gh >/dev/null && gh auth status >/dev/null 2>&1 \
     && sha=$(gh api "repos/$repo/commits/$tag" --jq .sha 2>/dev/null) && [ -n "$sha" ]; then
    echo "$sha"
    return 0
  fi
  sha=$(curl -fsSL -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$repo/commits/$tag" 2>/dev/null | jq -r '.sha // empty')
  [ -n "$sha" ] || return 1
  echo "$sha"
}
rev=$(resolve_rev) || { echo "cannot resolve $tag to a commit: mirror $mirror lacks it and api.github.com did not answer" >&2; exit 1; }
[[ "$rev" =~ ^[0-9a-f]{40}$ ]] || { echo "'$rev' is not a commit id" >&2; exit 1; }
echo "abgen $tag = $rev"

discover() {
  local name=$1 f
  while IFS= read -r -d '' f; do
    case "$f" in target/*|*/target/*|node_modules/*|*/node_modules/*) continue ;; esac
    [ -f "$one/$f" ] || continue
    echo "$one/$f"
  done < <(git -C "$one" ls-files -z --cached --others --exclude-standard -- "$name" "*/$name" | sort -zu)
}

url_re='"[^"]*decentraland/abgen[^"]*"'
line_re='^([[:space:]]*(inputs\.)?[A-Za-z0-9_-]+\.url[[:space:]]*=[[:space:]]*"github:decentraland/abgen/)[^"]*(";[[:space:]]*)$'
nix_files=()
while IFS= read -r f; do
  grep -qE "$url_re" "$f" || continue
  nix_files+=("$f")
  total=$(grep -cE "$url_re" "$f" || true)
  shaped=$(grep -cE "$line_re" "$f" || true)
  if [ "$total" != "$shaped" ]; then
    echo "${f#"$one/"}: an abgen url is not on a plain '<name>.url = \"github:$repo/<tag>\";' line -- move it there so this script can pin it" >&2
    exit 1
  fi
  sed -E "s#$line_re#\\1$tag\\3#" "$f" > "$f.new"
  if cmp -s "$f.new" "$f"; then
    rm -f "$f.new"
  else
    mv "$f.new" "$f"
    echo "wrote ${f#"$one/"}"
  fi
done < <(discover flake.nix)

JQ_STALE="$(cat <<'JQ'
. as $doc
| def reach($key; $keys; $names):
    if any($keys[]; . == $key) then []
    else
      ($doc.nodes[$key] // {}) as $n
      | (if ($n.locked.owner? == "decentraland" and $n.locked.repo? == "abgen"
            and (($n.locked.rev // "") != $rev or ($n.original.ref // "") != $tag))
         then [{input: $names[0], depth: ($names | length)}] else [] end)
        + ([ ($n.inputs // {}) | to_entries[] | select(.value | type == "string")
             | reach(.value; $keys + [$key]; $names + [.key]) ] | add // [])
    end;
  reach($doc.root; []; [])
| group_by(.input) | map({input: .[0].input, depth: (map(.depth) | min)})[]
| [.depth, .input] | @tsv
JQ
)"

lock_files=()
stale=()
while IFS= read -r f; do
  lock_files+=("$f")
  while IFS=$'\t' read -r depth input; do
    [ -n "$input" ] || continue
    stale+=("$depth"$'\t'"$(dirname "$f")"$'\t'"$input")
  done < <(jq -r --arg tag "$tag" --arg rev "$rev" "$JQ_STALE" "$f")
done < <(discover flake.lock)

if [ "${#stale[@]}" = 0 ]; then
  echo "every flake.lock already carries $tag = $rev"
else
  command -v nix >/dev/null || { echo "pin-abgen needs nix on PATH to re-lock ${#stale[@]} input(s)" >&2; exit 1; }
  while IFS=$'\t' read -r _ dir input; do
    echo "nix flake update $input --flake ${dir#"$one/"}"
    nix flake update "$input" --flake "$dir"
  done < <(printf '%s\n' "${stale[@]}" | sort -n -k1,1 -k2,2)
fi

echo "abgen pins now:"
printf '  %-58s version = %s\n' "${lock#"$one/"}" "$(sed -n 's/^version[[:space:]]*=[[:space:]]*//p' "$lock")"
for f in "${nix_files[@]}"; do
  while IFS= read -r line; do
    printf '  %-58s %s\n' "${f#"$one/"}:${line%%:*}" "${line#*:}"
  done < <(grep -noE "$url_re" "$f")
done
for f in "${lock_files[@]}"; do
  while IFS=$'\t' read -r key rev_now ref_now; do
    [ -n "$key" ] || continue
    printf '  %-58s %s %s\n' "${f#"$one/"} $key" "$rev_now" "$ref_now"
  done < <(jq -r '.nodes | to_entries[] | select(.value.locked.owner? == "decentraland" and .value.locked.repo? == "abgen") | [.key, (.value.locked.rev // ""), (.value.original.ref // "")] | @tsv' "$f")
done

gate_status=0
"$gate" || gate_status=$?
exit "$gate_status"
