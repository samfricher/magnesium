// Links in a reply that point at a file in the study folder.
//
// The tutor cites a passage as a Markdown link whose text is the file's name
// and page, and whose target is the path from the study folder with a page
// fragment, such as `[Week 4 slides, p. 12](Study/stats/week4-slides.pdf#page=12)`.
// The window opens such a link beside the lesson, after the learner has
// answered. Nothing here reads a file: the core does, inside the study folder.

export type SourceRef = { path: string; page: number | null };

/** The kinds of file the window shows itself, over the lesson: a PDF, text (Markdown drawn as prose), an image. Never handed to another app. */
const SHOWN = ["pdf", "txt", "md", "markdown", "tex", "csv", "json", "rst", "png", "jpg", "jpeg", "gif", "webp"];

/** Whether a link goes to a file on this Mac, and not to the web or inside the page. */
export function isLocalHref(href: string): boolean {
  if (href === "" || href.startsWith("#")) return false;
  return !/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("file:");
}

function decode(text: string): string {
  try {
    return decodeURI(text);
  } catch {
    return text;
  }
}

/** The file and the page a link names, or null when it is not a link to a file the panel shows. */
export function sourceRef(href: string): SourceRef | null {
  if (!isLocalHref(href)) return null;
  const bare = href.replace(/^file:\/\//, "");
  const hash = bare.indexOf("#");
  const path = decode(hash === -1 ? bare : bare.slice(0, hash)).trim();
  if (path === "") return null;
  const ext = path.includes(".") ? (path.split(".").pop() ?? "").toLowerCase() : "";
  if (!SHOWN.includes(ext)) return null;
  const page = hash === -1 ? null : /(?:^|&)page=(\d{1,5})(?:&|$)/.exec(bare.slice(hash + 1))?.[1];
  const number = page ? Number(page) : null;
  return { path, page: number !== null && number >= 1 ? number : null };
}
