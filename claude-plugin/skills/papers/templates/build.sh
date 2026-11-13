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
# LaTeX cannot break a line inside \text{} or a display, so such a line can
# run past the right edge of the page, and every PDF viewer cuts it off. A
# printer also leaves out about the last quarter inch of the page. After each
# build the script reads tectonic's log. A line that ends past the page, or
# within a quarter inch of its edge, is a CUT LINE: the script names it and
# does not write that PDF. A line that runs less far into the margin is only
# noted.
#
# Run it with bash from any directory, because the copied file is not
# executable:
#   bash Study/<target>/papers/src/build.sh            builds every paper
#   bash Study/<target>/papers/src/build.sh Diagnostic builds one paper
# It exits non-zero when tectonic is missing, any build fails or any line runs
# past the page.

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

# write_wrapper NAME VARIANT: the file tectonic builds.
#
# The comment package writes a key's solution text to its own file and inputs
# that, so TeX would number those lines from 1. \PrepareCutFile is the
# package's hook for the start of that file: one % line for each body line up
# to \begin{solution} keeps every line number in the log a line of the body.
#
# The first \typeout line puts the right margin, from the text's edge to the
# page's, in the log, so the check below follows any geometry the preamble
# sets. The second marks where the body ends: TeX sets a body's last paragraph
# only at \end{document}, and then numbers its last line in this file. The %
# keeps that \typeout line from adding a space to the paragraph.
write_wrapper() {
  if [ "$2" = key ]; then
    printf '%s\n' '\def\SHOWKEY{1}'
  fi
  cat <<'TEX'
\input{preamble}
\makeatletter
\ifdefined\PrepareCutFile
  \def\PrepareCutFile{\count@=\inputlineno
    \loop\ifnum\count@>\z@
      \immediate\write\CommentStream{\@percentchar}\advance\count@\m@ne
    \repeat}
\fi
\makeatother
\begin{document}
\typeout{build.sh right margin: \the\dimexpr\paperwidth-1in-\hoffset-\oddsidemargin-\textwidth\relax}
TEX
  printf '\\input{%s-body}\n\\typeout{build.sh body read}%%\n\\end{document}\n' "$1"
}

# check_lines LOG PDF BODY: name each line that TeX set wider than the text,
# once, with its biggest overrun. Cut lines go to stderr and make it return 1.
check_lines() {
  awk -v pdf="$2" -v body="$3" '
    /^build\.sh right margin: / {
      m = $0; sub(/^build\.sh right margin: /, "", m); margin = m + 0; seen = 1; next
    }
    /^build\.sh body read$/ { read = 1; next }
    seen && /^Overfull \\hbox \(/ {
      o = $0; sub(/^Overfull \\hbox \(/, "", o)
      if ($0 ~ /while \\output is active/) {
        where = "the page header or footer"
      } else if (match($0, /lines [0-9]+--[0-9]+/)) {
        split(substr($0, RSTART + 6, RLENGTH - 6), n, "--")
        if (read) where = "lines " n[1] " to the end of " body
        else where = (n[1] == n[2] ? "line " n[1] : "lines " n[1] " to " n[2]) " of " body
      } else if (match($0, /line [0-9]+/)) {
        where = substr($0, RSTART, RLENGTH) " of " body
      } else {
        where = "a line of " body
      }
      if (!(where in most)) { order[++count] = where; most[where] = 0 }
      if (o + 0 > most[where]) most[where] = o + 0
    }
    END {
      if (!seen) {
        print "build.sh: the log has no right margin line, so the lines of " pdf " were not checked" > "/dev/stderr"
        exit 2
      }
      # A quarter inch is 18.07pt.
      cut = 0
      for (i = 1; i <= count; i++) {
        where = order[i]
        left = margin - most[where]
        if (left < 18.07) {
          if (left < 0) {
            printf("CUT LINE in %s: %s is %.1fpt too wide, so it runs %.1fpt past the right edge of the page.\n",
              pdf, where, most[where], -left) > "/dev/stderr"
          } else {
            printf("CUT LINE in %s: %s is %.1fpt too wide, so it ends %.1fpt from the right edge of the page, where a printer cuts it off.\n",
              pdf, where, most[where], left) > "/dev/stderr"
          }
          cut = 1
        } else {
          printf("wide line in %s: %s runs %.1fpt into the %.1fpt right margin and stays on the page.\n",
            pdf, where, most[where], margin)
        }
      }
      exit cut
    }
  ' "$1"
}

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
    else
      wrapper=".build-$name"
      target="$out/$name.pdf"
    fi
    write_wrapper "$name" "$variant" >"$src/$wrapper.tex"
    if (cd "$src" && tectonic --chatter minimal --keep-logs --outdir "$work" "$wrapper.tex") \
      >"$work/$wrapper.out" 2>&1; then
      check_lines "$work/$wrapper.log" "$(basename "$target")" "$name-body.tex"
      case "$?" in
        0)
          mv "$work/$wrapper.pdf" "$target"
          echo "built $target"
          ;;
        1)
          {
            echo "FAILED: $name ($variant): $(basename "$target") was not written."
            printf '%s\n' 'Rewrite each CUT LINE so LaTeX can break it: end the math before its words, so they are text and not \text{}, and break a long display into lines with \\. Then run build.sh again.'
          } >&2
          fail=1
          ;;
        *)
          echo "FAILED: $name ($variant): $(basename "$target") was not written." >&2
          fail=1
          ;;
      esac
    else
      echo "FAILED: $name ($variant)" >&2
      grep -m 8 -E '^!|error' "$work/$wrapper.out" >&2
      fail=1
    fi
  done
done

exit "$fail"
