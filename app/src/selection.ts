import { withoutOpenMath } from "./render";
import { taskRange } from "./taskRule";

const QUOTE_MAX = 600;
const BLOCK = /^(P|LI|UL|OL|DIV|H[1-6]|BLOCKQUOTE|PRE|TABLE|TR|TD|TH|FIGURE|HR|BR)$/;
const LABEL = "p.task > strong:first-child";

export function selecting(): boolean {
  const selection = window.getSelection();
  return selection !== null && !selection.isCollapsed && selection.toString().trim() !== "";
}

export function selectedWords(range: Range, skip?: string): string {
  const out: string[] = [];
  const walk = (node: Node) => {
    if (!range.intersectsNode(node)) return;
    if (node.nodeType === Node.TEXT_NODE) {
      const text = node.textContent ?? "";
      out.push(text.slice(node === range.startContainer ? range.startOffset : 0, node === range.endContainer ? range.endOffset : text.length));
      return;
    }
    if (!(node instanceof Element) || (skip && node.matches(skip))) return;
    const tex = node.classList.contains("katex") ? node.querySelector('annotation[encoding="application/x-tex"]')?.textContent : null;
    if (tex) {
      out.push(node.closest(".katex-display") ? ` $$${tex}$$ ` : `$${tex}$`);
      return;
    }
    if (BLOCK.test(node.tagName)) out.push(" ");
    node.childNodes.forEach(walk);
  };
  const common = range.commonAncestorContainer;
  const holder = common instanceof Element ? common : common.parentElement;
  walk(holder?.closest(".katex") ?? common);
  return out.join("").replace(/\s+/g, " ").trim();
}

export function quotable(selected: string): string {
  const flat = selected.replace(/\s+/g, " ").trim();
  if (flat.length <= QUOTE_MAX) return flat;
  const cut = flat.slice(0, QUOTE_MAX);
  return `${withoutOpenMath(cut.slice(0, Math.max(cut.lastIndexOf(" "), QUOTE_MAX - 40))).trimEnd()}…`;
}

const words = (text: string) => text.toLowerCase().match(/[\p{L}\p{N}]+/gu)?.join(" ") ?? "";

function overlap(a: Range, b: Range): Range | null {
  if (a.compareBoundaryPoints(Range.END_TO_START, b) >= 0 || a.compareBoundaryPoints(Range.START_TO_END, b) <= 0) return null;
  const shared = document.createRange();
  const startsInA = a.compareBoundaryPoints(Range.START_TO_START, b) >= 0;
  shared.setStart(startsInA ? a.startContainer : b.startContainer, startsInA ? a.startOffset : b.startOffset);
  const endsInA = a.compareBoundaryPoints(Range.END_TO_END, b) <= 0;
  shared.setEnd(endsInA ? a.endContainer : b.endContainer, endsInA ? a.endOffset : b.endOffset);
  return shared;
}

export function inTask(range: Range, task: string): boolean {
  const node = range.commonAncestorContainer;
  const element = node instanceof Element ? node : node.parentElement;
  if (!element) return false;
  if (element.closest(".answer-task, .foot .parked")) return true;
  const prose = element.closest<HTMLElement>(".turn.open .tutor .prose");
  if (!prose) return false;
  const holder = prose.querySelector("div.task");
  const whole = holder ? document.createRange() : taskRange(prose, task);
  if (holder) whole?.selectNodeContents(holder);
  const shared = whole ? overlap(range, whole) : null;
  if (!shared) return false;
  const said = words(selectedWords(range, LABEL));
  return said !== "" && said === words(selectedWords(shared, LABEL));
}
