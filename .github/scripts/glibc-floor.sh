#!/usr/bin/env bash
# Fail when a shared object under DIR needs a glibc newer than MAX.
# usage: glibc-floor.sh <dir> [max-glibc-version]
set -euo pipefail

dir=${1:?usage: glibc-floor.sh <dir> [max-glibc-version]}
max=${2:-2.35}

highest=$(
  find "$dir" -type f \( -name '*.so' -o -name '*.so.*' \) -exec objdump -T {} + 2>/dev/null |
    grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/^GLIBC_//' | sort -uV | tail -n1 || true
)

if [[ -z $highest ]]; then
  echo "glibc-floor: no glibc symbols found under $dir" >&2
  exit 1
fi

if [[ $(printf '%s\n%s\n' "$max" "$highest" | sort -V | tail -n1) != "$max" ]]; then
  echo "glibc-floor: $dir requires GLIBC_$highest, above the GLIBC_$max floor" >&2
  exit 1
fi

echo "glibc-floor: highest requirement is GLIBC_$highest, floor GLIBC_$max"
