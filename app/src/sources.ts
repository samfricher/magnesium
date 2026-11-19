export type SourceRef = { path: string; page: number | null };

const SHOWN = ["pdf", "txt", "md", "markdown", "tex", "csv", "json", "rst", "png", "jpg", "jpeg", "gif", "webp"];

export function isLocalHref(href: string): boolean {
  if (href === "" || href.startsWith("#")) return false;
  return !/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("file:");
}

function decode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

const SPLIT = new RegExp(`^(.*?\\.(?:${SHOWN.join("|")}))(?:#(.*))?$`, "i");

export function sourceRef(href: string): SourceRef | null {
  if (!isLocalHref(href)) return null;
  const found = SPLIT.exec(decode(href.replace(/^file:\/\//, "")).trim());
  if (!found) return null;
  const path = found[1].trim();
  if (path === "" || /^\.[a-z0-9]+$/i.test(path)) return null;
  const page = found[2] ? /(?:^|&)page=(\d{1,5})(?:&|$)/.exec(found[2])?.[1] : null;
  const number = page ? Number(page) : null;
  return { path, page: number !== null && number >= 1 ? number : null };
}
