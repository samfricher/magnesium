#!/usr/bin/env bash
# Build every paper and its key with tectonic.
#
# Layout, under Study/<target>/papers/:
#   src/preamble.tex     the shared preamble; \SHOWKEY switches solutions on
#   src/build.sh         this script
#   src/<Name>-body.tex  one body per paper: \paperheader, then the questions,
#                        each solution in a solution environment
#   <Name>.pdf           the paper, built from <Name>-body.tex
#   <Name>-KEY.pdf       the key, built from the same body with \SHOWKEY
#
# The script writes the two wrapper files for each paper, builds both PDFs,
# and deletes the wrappers. Do not write wrapper files by hand.
#
# Run it with bash from any directory, because the copied file is not
# executable:
#   bash Study/<target>/papers/src/build.sh            builds every paper
#   bash Study/<target>/papers/src/build.sh Diagnostic builds one paper
# It exits non-zero when tectonic is missing or any build fails.

set -u

src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$(dirname "$src")"

if ! command -v tectonic >/dev/null 2>&1; then
  echo "build.sh: tectonic is not on PATH" >&2
  exit 127
fi

names=()
if [ "$#" -gt 0 ]; then
  names=("$@")
else
  for body in "$src"/*-body.tex; do
    [ -e "$body" ] || continue
    names+=("$(basename "$body" -body.tex)")
  done
fi
if [ "${#names[@]}" -eq 0 ]; then
  echo "build.sh: no <Name>-body.tex files in $src" >&2
  exit 1
fi

work="$(mktemp -d)"
cleanup() {
  rm -rf "$work"
  rm -f "$src"/.build-*.tex
}
trap cleanup EXIT

fail=0
for name in "${names[@]}"; do
  if [ ! -f "$src/$name-body.tex" ]; then
    echo "build.sh: missing $src/$name-body.tex" >&2
    fail=1
    continue
  fi
  for variant in paper key; do
    if [ "$variant" = key ]; then
      wrapper=".build-$name-KEY"
      target="$out/$name-KEY.pdf"
      keyline='\def\SHOWKEY{1}'
    else
      wrapper=".build-$name"
      target="$out/$name.pdf"
      keyline=''
    fi
    printf '%s\n\\input{preamble}\n\\begin{document}\n\\input{%s-body}\n\\end{document}\n' \
      "$keyline" "$name" >"$src/$wrapper.tex"
    if (cd "$src" && tectonic --chatter minimal --outdir "$work" "$wrapper.tex") \
      >"$work/$wrapper.log" 2>&1; then
      mv "$work/$wrapper.pdf" "$target"
      echo "built $target"
    else
      echo "FAILED: $name ($variant)" >&2
      grep -m 8 -E '^!|error' "$work/$wrapper.log" >&2
      fail=1
    fi
  done
done

exit "$fail"
