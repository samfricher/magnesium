// The answer written on Home under the first quick check's question, kept for this run of the window until it has
// been sent. Return on Home starts a session that may never send the words (it failed to open, or what it posed was
// not that question): until they are sent, Home still holds them, so they are never lost. The session that sends them
// clears them here, and Home's line reads them from here again when it comes to the front.

export type HomeDraft = { folder: string; question: string; words: string };

let kept: HomeDraft | null = null;

/** The words written on Home for this folder's question, if any. */
export function homeDraftFor(folder: string, question: string): string {
  return kept && kept.folder === folder && kept.question === question ? kept.words : "";
}

export function keepHomeDraft(draft: HomeDraft): void {
  kept = draft.words.trim() === "" ? null : draft;
}

/** These words, for this folder's question, have gone to the tutor, from the session that took them or from its line. */
export function sentHomeDraft(folder: string, question: string, words: string): void {
  if (kept && kept.folder === folder && kept.question === question && kept.words.trim() === words.trim()) kept = null;
}
