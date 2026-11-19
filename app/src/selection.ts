import { taskRange } from "./taskRule";

export function selecting(): boolean {
  const selection = window.getSelection();
  return selection !== null && !selection.isCollapsed && selection.toString().trim() !== "";
}

const BLOCK = /^(P|LI|UL|OL|DIV|H[1-6]|BLOCKQUOTE|PRE|TABLE|TR|TD|TH|FIGURE|HR|BR)$/;

export function selectedWords(range: Range): string {
  const out: string[] = [];
  const walk = (node: Node) => {
    if (!range.intersectsNode(node)) return;
    if (node.nodeType === Node.TEXT_NODE) {
      const text = node.textContent ?? "";
      out.push(text.slice(node === range.startContainer ? range.startOffset : 0, node === range.endContainer ? range.endOffset : text.length));
      return;
    }
    if (!(node instanceof Element)) return;
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

export function inTask(range: Range, task: string): boolean {
  const node = range.commonAncestorContainer;
  const element = node instanceof Element ? node : node.parentElement;
  if (!element) return false;
  if (element.closest(".answer-task, .foot .parked, .turn.open .prose div.task")) return true;
  const prose = element.closest<HTMLElement>(".turn.open .tutor .prose");
  if (!prose) return false;
  const whole = taskRange(prose, task);
  return whole !== null && whole.compareBoundaryPoints(Range.START_TO_START, range) <= 0 && whole.compareBoundaryPoints(Range.END_TO_END, range) >= 0;
}
