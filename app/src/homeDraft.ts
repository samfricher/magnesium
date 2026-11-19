export type HomeDraft = { folder: string; question: string; words: string };

let kept: HomeDraft | null = null;

export function homeDraftFor(folder: string, question: string): string {
  return kept && kept.folder === folder && kept.question === question ? kept.words : "";
}

export function keepHomeDraft(draft: HomeDraft): void {
  kept = draft.words.trim() === "" ? null : draft;
}

export function sentHomeDraft(folder: string, question: string, words: string): void {
  if (kept && kept.folder === folder && kept.question === question && kept.words.trim() === words.trim()) kept = null;
}
