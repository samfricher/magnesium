// The answer written on Home under the first quick check's question, kept for this run of the window until it has
// been sent. Home is drawn again each time it comes to the front, and Return on it starts a session that may never
// send the words (it failed to open, or what it posed was not that question): until they are sent, Home still holds
// them, so they are never lost.

export type HomeDraft = { folder: string; question: string; words: string };

let kept: HomeDraft | null = null;

/** The words written on Home for this folder's question, if any. */
export function homeDraftFor(folder: string, question: string): string {
  return kept && kept.folder === folder && kept.question === question ? kept.words : "";
}

export function keepHomeDraft(draft: HomeDraft): void {
  kept = draft.words.trim() === "" ? null : draft;
}

/** The words have gone to the tutor, from the session that took them or from its line: Home lets them go. */
export function sentHomeDraft(text: string): void {
  if (kept && text.includes(kept.words.trim())) kept = null;
}
