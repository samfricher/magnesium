import { coverage } from "./render";
import { locate, ruleAction, taskPlain, type Piece } from "./taskLine";

const BLOCK = /^(P|LI|UL|OL|DIV|H[1-6]|BLOCKQUOTE|PRE|TABLE|TR|TD|TH|FIGURE|HR)$/;

function pieces(root: HTMLElement): { pieces: Piece[]; nodes: (Node | null)[] } {
  const out: Piece[] = [];
  const nodes: (Node | null)[] = [];
  const walk = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      out.push({ text: node.textContent ?? "", atomic: false });
      nodes.push(node);
      return;
    }
    if (!(node instanceof HTMLElement)) return;
    if (node.classList.contains("earlier")) return;
    if (node.classList.contains("katex")) {
      const tex = node.querySelector('annotation[encoding="application/x-tex"]')?.textContent ?? node.textContent ?? "";
      out.push({ text: tex, atomic: true });
      nodes.push(node);
      return;
    }
    if (node.tagName === "BR" || BLOCK.test(node.tagName) || node.classList.contains("katex-display")) {
      out.push({ text: " ", atomic: false });
      nodes.push(null);
    }
    node.childNodes.forEach(walk);
  };
  root.childNodes.forEach(walk);
  return { pieces: out, nodes };
}

export function taskRange(root: HTMLElement, task: string): Range | null {
  const { pieces: list, nodes } = pieces(root);
  const found = locate(list, taskPlain(task));
  if (!found) return null;
  const start = nodes[found.start.piece];
  const end = nodes[found.end.piece];
  if (!start || !end) return null;
  const range = document.createRange();
  if (found.start.at === -1) range.setStartBefore(start);
  else range.setStart(start, found.start.at);
  if (found.end.at === -1) range.setEndAfter(end);
  else range.setEnd(end, found.end.at);
  return range;
}

function blockRange(root: HTMLElement, task: string): Range | null {
  const blocks = Array.from(root.children);
  let text = "";
  for (let at = blocks.length - 1; at >= 0; at--) {
    text = `${blocks[at].textContent ?? ""} ${text}`;
    if (coverage(text, task) >= 0.8) {
      const range = document.createRange();
      range.setStartBefore(blocks[at]);
      range.setEndAfter(blocks[blocks.length - 1]);
      return range;
    }
  }
  return null;
}

export type Box = { top: number; bottom: number };

function drawnIn(prose: HTMLElement, task: string, open: boolean): Box | null {
  if (open) {
    const block = prose.querySelector("div.task");
    if (block) return block.getBoundingClientRect();
    if (prose.classList.contains("has-task-rule")) {
      const top = prose.getBoundingClientRect().top + (parseFloat(prose.style.getPropertyValue("--task-top")) || 0);
      return { top, bottom: top + (parseFloat(prose.style.getPropertyValue("--task-height")) || 0) };
    }
  }
  const range = taskRange(prose, task);
  return range ? range.getBoundingClientRect() : null;
}

export function taskBox(page: HTMLElement, task: string): Box | null {
  const open = page.querySelector<HTMLElement>(".turn.open .tutor .prose");
  const there = open ? drawnIn(open, task, true) : null;
  if (there) return there;
  const proses = Array.from(page.querySelectorAll<HTMLElement>("article.turn .tutor .prose")).reverse();
  for (const prose of proses) {
    if (prose === open) continue;
    const box = drawnIn(prose, task, false);
    if (box) return box;
  }
  return null;
}

export function drawTaskRule(root: HTMLElement, task: string): () => void {
  const clear = () => {
    root.classList.remove("has-task-rule");
    root.style.removeProperty("--task-top");
    root.style.removeProperty("--task-height");
  };
  const draw = () => {
    const range = taskRange(root, task) ?? blockRange(root, task);
    const rects = range ? Array.from(range.getClientRects()).filter((rect) => rect.width > 0 && rect.height > 0) : [];
    const action = ruleAction(rects.length, rects.length === 0 && getComputedStyle(root).contentVisibility === "auto");
    if (action === "clear") return clear();
    if (action === "keep") return;
    const base = root.getBoundingClientRect();
    const top = Math.min(...rects.map((rect) => rect.top)) - base.top;
    const bottom = Math.max(...rects.map((rect) => rect.bottom)) - base.top;
    root.style.setProperty("--task-top", `${top}px`);
    root.style.setProperty("--task-height", `${bottom - top}px`);
    root.classList.add("has-task-rule");
  };
  draw();
  const watch = new ResizeObserver(draw);
  watch.observe(root);
  return () => {
    watch.disconnect();
    clear();
  };
}
