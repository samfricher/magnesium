// The plan is the plan file itself, Study/<plan>/plan.md, shown as a
// document. When the tutor changes it, the lines that changed carry a bar in
// the gutter and the lines that went are struck through where they were, as
// an editor shows a changed file. The learner can edit it as text (⌘E).

import { marked, type Tokens } from "marked";
import { memo, useEffect, useMemo, useRef, useState } from "react";

import { diffLines, type LineDiff } from "../diff";
import { renderInline, renderMarkdown } from "../render";
import type { Plan } from "../types";

/** Sections of the template that are reference, folded until opened. */
const FOLDED = /instructor|research|paper bank|method|tags|anchors|checklist|confirm|using this folder/i;

const lineCount = (text: string) => (text.match(/\n/g) ?? []).length;

type Block = {
  /** The first line of the block in the file. */
  start: number;
  token: Tokens.Generic;
};

type Section = { heading: Tokens.Heading | null; start: number; blocks: Block[] };

function sections(text: string): { title: string | null; sections: Section[] } {
  const tokens = marked.lexer(text);
  let line = 0;
  let title: string | null = null;
  const out: Section[] = [{ heading: null, start: 0, blocks: [] }];
  for (const token of tokens) {
    const start = line;
    line += lineCount(token.raw);
    if (token.type === "space") continue;
    if (token.type === "html" && /^\s*<!--/.test(token.raw)) continue;
    if (token.type === "heading" && token.depth === 1 && title === null) {
      title = (token as Tokens.Heading).text;
      continue;
    }
    if (token.type === "heading" && token.depth === 2) {
      out.push({ heading: token as Tokens.Heading, start, blocks: [] });
      continue;
    }
    out[out.length - 1].blocks.push({ start, token });
  }
  return { title, sections: out.filter((s) => s.heading !== null || s.blocks.length > 0) };
}

const changedIn = (diff: LineDiff | null, from: number, to: number) => {
  if (!diff) return false;
  for (let i = from; i < to; i++) if (diff.changed[i]) return true;
  return false;
};

const goneBefore = (diff: LineDiff | null, line: number) => diff?.gone.get(line) ?? [];

function cells(row: string): string[] {
  return row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((cell) => cell.trim());
}

function TableBlock({ block, diff }: { block: Block; diff: LineDiff | null }) {
  const token = block.token as Tokens.Table;
  const rowLine = (r: number) => block.start + 2 + r;
  return (
    <div className="table">
      <table>
        <thead>
          <tr>
            {token.header.map((cell, i) => (
              <th key={i} dangerouslySetInnerHTML={{ __html: renderInline(cell.text) }} />
            ))}
          </tr>
        </thead>
        <tbody>
          {token.rows.map((row, r) => (
            <RowWithGhosts key={r} line={rowLine(r)} diff={diff} width={token.header.length}>
              {row.map((cell, i) => (
                <td key={i} dangerouslySetInnerHTML={{ __html: renderInline(cell.text) }} />
              ))}
            </RowWithGhosts>
          ))}
          {goneBefore(diff, rowLine(token.rows.length))
            .filter((line) => line.trim().startsWith("|"))
            .map((line, g) => (
              <GhostRow key={`end-${g}`} line={line} width={token.header.length} />
            ))}
        </tbody>
      </table>
    </div>
  );
}

function GhostRow({ line, width }: { line: string; width: number }) {
  const parts = cells(line);
  if (/^[\s|:-]+$/.test(line)) return null;
  return (
    <tr className="gone" aria-label="Removed">
      {Array.from({ length: width }, (_, i) => (
        <td key={i}>
          <del dangerouslySetInnerHTML={{ __html: renderInline(parts[i] ?? "") }} />
        </td>
      ))}
    </tr>
  );
}

function RowWithGhosts({ line, diff, width, children }: { line: number; diff: LineDiff | null; width: number; children: React.ReactNode }) {
  const ghosts = goneBefore(diff, line).filter((text) => text.trim().startsWith("|"));
  return (
    <>
      {ghosts.map((text, g) => (
        <GhostRow key={g} line={text} width={width} />
      ))}
      <tr className={diff?.changed[line] ? "chg" : undefined}>{children}</tr>
    </>
  );
}

const BlockView = memo(function BlockView({ block, diff }: { block: Block; diff: LineDiff | null }) {
  const end = block.start + lineCount(block.token.raw);
  const ghosts = goneBefore(diff, block.start).filter((text) => !text.trim().startsWith("|"));
  const ghostView =
    ghosts.length > 0 ? (
      <div className="gone-lines" aria-label="Removed">
        {ghosts.map((text, g) => (
          <del key={g} dangerouslySetInnerHTML={{ __html: renderInline(text.replace(/^\s*[-*+]\s+|^\s*\d+\.\s+/, "")) }} />
        ))}
      </div>
    ) : null;
  if (block.token.type === "table") {
    return (
      <>
        {ghostView}
        <TableBlock block={block} diff={diff} />
      </>
    );
  }
  if (block.token.type === "list") {
    const list = block.token as Tokens.List;
    let line = block.start;
    const Tag = list.ordered ? "ol" : "ul";
    return (
      <>
        {ghostView}
        <Tag className="plan-list">
          {list.items.map((item, i) => {
            const from = line;
            line += lineCount(item.raw) || 1;
            const gone = i > 0 ? goneBefore(diff, from).filter((t) => !t.trim().startsWith("|")) : [];
            return (
              <li key={i} className={changedIn(diff, from, Math.max(from + 1, line)) ? "chg" : undefined}>
                {gone.map((text, g) => (
                  <del key={g} className="gone-item" dangerouslySetInnerHTML={{ __html: renderInline(text.replace(/^\s*[-*+]\s+|^\s*\d+\.\s+/, "")) }} />
                ))}
                <span dangerouslySetInnerHTML={{ __html: renderInline(item.text) }} />
              </li>
            );
          })}
        </Tag>
      </>
    );
  }
  return (
    <>
      {ghostView}
      <div className={`prose plan-block${changedIn(diff, block.start, Math.max(end, block.start + 1)) ? " chg" : ""}`} dangerouslySetInnerHTML={{ __html: renderMarkdown(block.token.raw) }} />
    </>
  );
});

export type PlanProps = {
  plan: Plan | null;
  /** The plan as it was before the tutor's last change, while its marks are shown. */
  before: string | null;
  changedAt: number | null;
  active: boolean;
  folds: Record<string, boolean>;
  onFold: (heading: string, open: boolean) => void;
  onSeen: () => void;
  onSave: (text: string) => Promise<string | null>;
  onTalk: () => void;
};

export function PlanDoc(props: PlanProps) {
  const { plan } = props;
  const text = plan?.text ?? null;
  const read = useMemo(() => (text ? sections(text) : null), [text]);
  const diff = useMemo(() => (text && props.before !== null && props.before !== text ? diffLines(props.before, text) : null), [text, props.before]);
  const [editing, setEditing] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const latest = useRef(props);
  latest.current = props;

  useEffect(() => {
    if (!props.active) return;
    const onKey = (event: KeyboardEvent) => {
      const command = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();
      if (command && key === "e" && latest.current.plan?.text) {
        event.preventDefault();
        setProblem(null);
        setEditing((old) => (old === null ? (latest.current.plan?.text ?? "") : null));
      } else if (command && key === "s" && area.current) {
        event.preventDefault();
        const value = area.current.value;
        void latest.current.onSave(value).then((failed) => {
          setProblem(failed);
          if (!failed) setEditing(null);
        });
      } else if (event.key === "Escape" && !event.defaultPrevented) {
        if (area.current) {
          event.preventDefault();
          setEditing(null);
        } else if (latest.current.before !== null) {
          latest.current.onSeen();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.active]);

  useEffect(() => {
    if (editing !== null) area.current?.focus();
  }, [editing !== null]);

  if (!plan || !text || !read) {
    return (
      <article className="plan" data-doc="plan">
        <h1 className="doc-title">Plan</h1>
        <p className="quiet-para">This folder has no plan yet. In a lesson, tell the tutor what you are preparing for and when, and it writes one here.</p>
      </article>
    );
  }

  if (editing !== null) {
    return (
      <article className="plan editing" data-doc="plan">
        <p className="doc-path">
          {plan.path} <span className="quiet">⌘S saves · Esc leaves it as it was</span>
        </p>
        {problem && <p className="notice">{problem}</p>}
        <textarea ref={area} className="source-edit" defaultValue={editing} spellCheck={false} aria-label="The plan file, as text" />
      </article>
    );
  }

  const changedTime = props.changedAt ? new Date(props.changedAt).toTimeString().slice(0, 5) : null;
  return (
    <article className="plan" data-doc="plan" data-changed={diff ? true : undefined}>
      <h1 className="doc-title" dangerouslySetInnerHTML={{ __html: renderInline(read.title ?? plan.title ?? "Plan") }} />
      <p className="doc-path">
        {plan.path}
        {diff && changedTime && <span className="changed-note"> · changed at {changedTime}; the bars mark what changed</span>}
      </p>
      {read.sections.map((section, s) => {
        const name = section.heading?.text ?? "";
        const folded = section.heading ? !(props.folds[name] ?? !FOLDED.test(name)) : false;
        const end = read.sections[s + 1]?.start ?? Number.MAX_SAFE_INTEGER;
        const touched = changedIn(diff, section.start, Math.min(end, diff?.changed.length ?? 0));
        const shownFolded = folded && !touched;
        return (
          <section key={`${name}-${s}`} className="plan-section" data-folded={shownFolded || undefined}>
            {section.heading && (
              <h2>
                <button type="button" className="fold" aria-expanded={!shownFolded} onClick={() => props.onFold(name, shownFolded)}>
                  <span dangerouslySetInnerHTML={{ __html: renderInline(name) }} />
                  {shownFolded && <span className="folded-note">{section.blocks.length > 0 ? " …" : ""}</span>}
                </button>
              </h2>
            )}
            {!shownFolded && section.blocks.map((block) => <BlockView key={block.start} block={block} diff={diff} />)}
          </section>
        );
      })}
      <p className="plan-foot">
        <button type="button" className="text-button" onClick={props.onTalk}>
          Change it by telling the tutor
        </button>
        <span className="quiet"> · or edit it yourself with ⌘E</span>
      </p>
    </article>
  );
}
