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
# LaTeX cannot break a line inside \text{}, \mbox{} or a display, so such a
# line can be set wider than its column and run past the right edge of the
# page, where every PDF viewer cuts it off. A printer also leaves out about the
# last quarter inch of the page. After each build the script finds where each
# too-wide line ends on the page:
#   CUT LINE        ends past the page, or within a quarter inch of its edge.
#                   The script does not write that PDF.
#   UNCHECKED LINE  too wide, but the script cannot tell where it ends. The
#                   script does not write that PDF either.
#   wide line       ends in the margin, or inside the text where it may cover
#                   what is beside it. Only noted; the PDF is written.
#
# Run it with bash from any directory, because the copied file is not
# executable:
#   bash Study/<target>/papers/src/build.sh            builds every paper
#   bash Study/<target>/papers/src/build.sh Diagnostic builds one paper
# It exits non-zero when tectonic is missing, any build fails, or a paper has
# a CUT LINE or an UNCHECKED LINE.

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
# \showboxdepth=0 makes the log give the size of each box a too-wide line is
# set in, which the check matches to the box's place on the page.
#
# The first \typeout line puts the page width and the x of the text's right
# edge in the log, so the check follows any geometry the preamble sets. The
# second marks where the body ends: TeX sets a body's last paragraph only at
# \end{document}, and then numbers its last line in this file. The % keeps
# that \typeout line from adding a space to the paragraph.
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
\showboxdepth=0
\typeout{build.sh page: \the\paperwidth\space\the\dimexpr1in+\hoffset+\oddsidemargin+\textwidth\relax}
TEX
  printf '\\input{%s-body}\n\\typeout{build.sh body read}%%\n\\end{document}\n' "$1"
}

# check_lines LOG SYNCTEX PDF BODY: say where each too-wide line ends.
#
# TeX's log gives how much wider a line is than the box it is set in (a line
# of the text, a minipage, a table cell), that box's size, and the body line.
# It does not give where the box is. SyncTeX records every box on the page
# with its body line, size and left edge, so the box with the same line and
# size gives the place: the line ends at its left edge plus its width plus the
# overrun. With no such box, or several in different places, the line is
# UNCHECKED. Lines go out once per place in the body, at their furthest end.
# Returns 1 when any line is CUT or UNCHECKED, 2 when the log or SyncTeX data
# is missing.
check_lines() {
  awk -v pdf="$3" -v body="$4" '
    function sp(x) { return int(x * 65536 + (x < 0 ? -0.5 : 0.5)) }
    function pt(x) { return x / 65536 }
    # Settle the pending report: its kind and how far right it ends.
    function settle(    key, k, at, end) {
      if (!pending) return
      pending = 0
      key = dims == "" ? "" : (lineno == "" ? dims : lineno SUBSEP dims)
      if (key != "" && (key in lo) && hi[key] - lo[key] <= 32768) {
        end = pt(hi[key]) + width + over
        if (end > paper - 18.07) k = "cut"
        else if (end > right) k = "margin"
        else k = "inside"
      } else {
        k = "unchecked"
        end = 0
      }
      at = where SUBSEP k
      if (!(at in far)) { order[++count] = at; far[at] = end; most[at] = over }
      if (end > far[at]) far[at] = end
      if (over > most[at]) most[at] = over
    }
    FILENAME == ARGV[1] {
      if ($0 ~ /^Unit:/) unit = substr($0, 6) + 0
      if ($0 !~ /^\(/) next
      if (!unit) unit = 1
      split(substr($0, 2), f, /[,:]/)
      h = f[3] * unit; d = (f[5] * unit) SUBSEP (f[6] * unit) SUBSEP (f[7] * unit)
      synced = 1
      for (i = 1; i <= 2; i++) {
        key = i == 1 ? f[2] SUBSEP d : d
        if (!(key in lo) || h < lo[key]) lo[key] = h
        if (!(key in hi) || h > hi[key]) hi[key] = h
      }
      next
    }
    /^build\.sh page: / {
      split($0, f, " "); paper = f[3] + 0; right = f[4] + 0; seen = 1; next
    }
    /^build\.sh body read$/ { settle(); read = 1; next }
    seen && /^Overfull \\hbox \(/ {
      settle()
      o = $0; sub(/^Overfull \\hbox \(/, "", o); over = o + 0
      lineno = ""; dims = ""; plural = 0
      if ($0 ~ /while \\output is active/) {
        where = "the page header or footer"
      } else if (match($0, /lines [0-9]+--[0-9]+/)) {
        split(substr($0, RSTART + 6, RLENGTH - 6), n, "--")
        lineno = n[2]
        if (read) { where = "lines " n[1] " to the end of " body; plural = 1 }
        else if (n[1] == n[2]) where = "line " n[1] " of " body
        else { where = "lines " n[1] " to " n[2] " of " body; plural = 1 }
      } else if (match($0, /line [0-9]+/)) {
        lineno = substr($0, RSTART + 5, RLENGTH - 5)
        where = "line " lineno " of " body
      } else {
        where = "a line of " body
      }
      many[where] = plural
      pending = 1
      next
    }
    pending && /^\\hbox\(/ {
      s = $0; sub(/^\\hbox\(/, "", s)
      p = index(s, "+"); hh = substr(s, 1, p - 1); s = substr(s, p + 1)
      p = index(s, ")x"); dd = substr(s, 1, p - 1); width = substr(s, p + 2) + 0
      dims = sp(width) SUBSEP sp(hh + 0) SUBSEP sp(dd + 0)
      settle()
      next
    }
    END {
      settle()
      if (!seen || !synced) {
        print "build.sh: the log or the SyncTeX data is missing, so the lines of " pdf " were not checked" > "/dev/stderr"
        exit 2
      }
      bad = 0
      for (i = 1; i <= count; i++) {
        split(order[i], at, SUBSEP)
        w = at[1]; k = at[2]; end = far[order[i]]; a = most[order[i]]
        if (many[w]) { is = "are"; its = "their"; it = "they"; s = "" }
        else { is = "is"; its = "its"; it = "it"; s = "s" }
        if (k == "cut" && end > paper) {
          printf("CUT LINE in %s: %s %s %.1fpt too wide for %s column, so %s run%s %.1fpt past the right edge of the page.\n",
            pdf, w, is, a, its, it, s, end - paper) > "/dev/stderr"
        } else if (k == "cut") {
          printf("CUT LINE in %s: %s %s %.1fpt too wide for %s column, so %s end%s %.1fpt from the right edge of the page, where a printer cuts %s off.\n",
            pdf, w, is, a, its, it, s, paper - end, many[w] ? "them" : "it") > "/dev/stderr"
        } else if (k == "unchecked") {
          printf("UNCHECKED LINE in %s: %s %s %.1fpt too wide for %s column or cell, and build.sh cannot tell where on the page %s end%s.\n",
            pdf, w, is, a, its, it, s) > "/dev/stderr"
        } else if (k == "margin") {
          printf("wide line in %s: %s %s %.1fpt too wide for %s column, so %s run%s %.1fpt into the right margin and stay%s on the page.\n",
            pdf, w, is, a, its, it, s, end - right, s)
        } else {
          printf("wide line in %s: %s %s %.1fpt too wide for %s column but end%s %.1fpt short of the right margin. Check that %s cover%s nothing beside %s.\n",
            pdf, w, is, a, its, s, right - end, it, s, many[w] ? "them" : "it")
        }
        if (k == "cut" || k == "unchecked") bad = 1
      }
      exit bad
    }
  ' "$2" "$1"
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
    if (cd "$src" && tectonic --chatter minimal --keep-logs --synctex --outdir "$work" "$wrapper.tex") \
      >"$work/$wrapper.out" 2>&1; then
      gzip -dc "$work/$wrapper.synctex.gz" >"$work/$wrapper.synctex" 2>/dev/null
      check_lines "$work/$wrapper.log" "$work/$wrapper.synctex" "$(basename "$target")" "$name-body.tex"
      case "$?" in
        0)
          mv "$work/$wrapper.pdf" "$target"
          echo "built $target"
          ;;
        1)
          {
            echo "FAILED: $name ($variant): $(basename "$target") was not written."
            printf '%s\n' 'Rewrite each line named above so it fits its column. Let LaTeX break it: put words in text, not in \text{} or \mbox{}, and split a long display into lines with \\. In a table, shorten the cell or widen its column. Then run build.sh again.'
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
