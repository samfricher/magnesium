// Which lines of a file changed, as an editor's gutter shows it: each line of
// the new text is kept or changed, and the old lines that went are attached
// to the new line they stood before.

export type LineDiff = {
  /** For each new line: true when it is new or changed. */
  changed: boolean[];
  /** Old lines that went, by the index of the new line they stood before (the end is `changed.length`). */
  gone: Map<number, string[]>;
};

/** A line-by-line diff by longest common subsequence. Plans are short, so the plain table is fine. */
export function diffLines(before: string, after: string): LineDiff {
  const a = before.split("\n");
  const b = after.split("\n");
  const n = a.length;
  const m = b.length;
  // Too large to compare cheaply: say nothing changed rather than guess.
  if (n * m > 400_000) return { changed: b.map(() => false), gone: new Map() };
  const table: Uint16Array[] = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }
  const changed = b.map(() => true);
  const gone = new Map<number, string[]>();
  let i = 0;
  let j = 0;
  const drop = (line: string, at: number) => {
    if (line.trim() === "") return;
    gone.set(at, [...(gone.get(at) ?? []), line]);
  };
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      changed[j] = false;
      i++;
      j++;
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      drop(a[i], j);
      i++;
    } else {
      j++;
    }
  }
  while (i < n) drop(a[i++], m);
  // A blank line that came or went is not a change worth a mark.
  b.forEach((line, k) => {
    if (line.trim() === "") changed[k] = false;
  });
  return { changed, gone };
}
