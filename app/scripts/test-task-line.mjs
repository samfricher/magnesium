// Checks for finding a task's words in a drawn reply (src/taskLine.ts), so
// the line beside the task can be drawn over the reply without moving it.
// Run by scripts/check.sh; node runs the TypeScript file as it is.
//
//   node scripts/test-task-line.mjs
//
// A drawn reply is given as pieces, as src/taskRule.ts reads them from the
// page: each text node's text, a space where a block begins, and each formula
// whole, as the TeX KaTeX keeps for it.

import assert from "node:assert/strict";
import { locate, taskPlain, texKey } from "../src/taskLine.ts";

const t = (text) => ({ text, atomic: false });
const f = (tex) => ({ text: tex, atomic: true });
const block = t(" ");
const find = (pieces, task) => locate(pieces, taskPlain(task));
const at = (piece, offset) => ({ piece, at: offset });

// The plain form of a task in markdown.
assert.equal(taskPlain("Now expand $(x + 4)(x + 5)$ and simplify."), "Now expand (x+4)(x+5) and simplify.");
assert.equal(taskPlain("**Expand** \\((x+1)^2\\), then *check* it."), "Expand (x+1)^2, then check it.");
assert.equal(taskPlain("Read [the notes](notes.md) and find $V_{max}$."), "Read the notes and find V_max.");
assert.equal(taskPlain("Expand\n\n$$\n(x+1)(x+2)\n$$\n\nand simplify."), "Expand (x+1)(x+2) and simplify.");
assert.equal(taskPlain("Solve \\begin{cases} x+y=3 \\\\ x-y=1 \\end{cases} for both."), "Solve \\begincasesx+y=3\\\\x-y=1\\endcases for both.");
assert.equal(taskPlain("It costs $5 and $10 here, so 2\\*3 is `a_b`."), "It costs $5 and $10 here, so 2*3 is a_b.");
assert.equal(taskPlain("**Quick check 2:** What is $b$ in $x^2 - 5x$?"), "What is b in x^2-5x?");
assert.equal(taskPlain("factorise $x^2 + 7x + 12$.**"), "factorise x^2+7x+12.", "a bold mark left open at the end");
assert.equal(taskPlain("snake_case stays as it is"), "snake_case stays as it is");
assert.equal(texKey("V_{\\text{max}} = 2 \\cdot x"), "V_max=2\\cdotx", "the page's upright subscripts compare as written");

// V_max as a tutor may write it, against what the page typesets: V_{\text{max}} for all three.
for (const written of ["$V_max$", "$V_{max}$", "$V_\\text{max}$", "$V_{\\text{max}}$"]) {
  const drawn = [t("Find "), f("V_{\\text{max}}"), t(" from the intercept.")];
  assert.deepEqual(find(drawn, `Find ${written} from the intercept.`), { start: at(0, 0), end: at(2, 20) }, written);
}
assert.deepEqual(find([t("So "), f("K_{\\text{m}}=2"), t(". Now find "), f("V_{\\text{max}}"), t(".")], "Now find $V_max$."), { start: at(2, 2), end: at(4, 1) });

// A task with maths that starts partway through a line of teaching.
const teaching = [
  t("To expand "), f("(x+2)(x+3)"), t(", multiply every term by every term to get "), f("x^2 + 5x + 6"),
  t(". Now expand "), f("(x + 4)(x + 5)"), t(" and simplify."),
];
assert.deepEqual(find(teaching, "Now expand $(x+4)(x+5)$ and simplify."), { start: at(4, 2), end: at(6, 14) });

// A task that ends in a formula, or begins with one.
assert.deepEqual(find(teaching, "Now expand $(x + 4)(x + 5)$"), { start: at(4, 2), end: at(5, -1) });
assert.deepEqual(find([t("So "), f("x^2+7x+12"), t(" is next: factorise it.")], "$x^2 + 7x + 12$ is next: factorise it."), { start: at(1, -1), end: at(2, 23) });

// Bold around and inside the task, and the task starting inside bold.
const bold = [t("Right. "), t("Now factorise"), t(" "), f("x^2+7x+12"), t(".")];
assert.deepEqual(find(bold, "**Now factorise** $x^2+7x+12$."), { start: at(1, 0), end: at(4, 1) });
assert.deepEqual(find([t("Your turn: factorise "), f("x^2+7x+12"), t(".")], "factorise $x^2+7x+12$.**"), { start: at(0, 11), end: at(2, 1) });

// A link, the page's upright subscripts, and a \( \) formula.
const linked = [t("Read "), t("the notes"), t(" and find "), f("V_{\\text{max}}"), t(" from "), f("\\frac{1}{2}"), t(".")];
assert.deepEqual(find(linked, "Read [the notes](notes.md) and find $V_{max}$ from \\(\\frac{1}{2}\\)."), { start: at(0, 0), end: at(6, 1) });

// A task over several blocks, with a display formula between them (drawn in a span, read as a block).
const blocks = [block, t("Teaching."), block, t("Expand"), block, f("(x+1)(x+2)"), block, t("and simplify.")];
assert.deepEqual(find(blocks, "Expand\n\n$$(x+1)(x+2)$$\n\nand simplify."), { start: at(3, 0), end: at(7, 13) });

// A quick check: its "Quick check 2:" is drawn as "From earlier", which is left out of the pieces.
assert.deepEqual(find([t(" What is "), f("b"), t(" in "), f("x^2 - 5x"), t("?")], "**Quick check 2:** What is $b$ in $x^2 - 5x$?"), { start: at(0, 1), end: at(4, 1) });

// The last place the words stand, when they stand twice.
assert.deepEqual(find([t("Expand it. Then: expand it.")], "expand it."), { start: at(0, 17), end: at(0, 27) });

// No-break spaces and line breaks count as spaces.
assert.deepEqual(find([t("Now expand"), t("\n"), f("x")], "Now expand $x$"), { start: at(0, 0), end: at(2, -1) });

// Nothing to find: other words, a different formula, or the task starting or stopping inside a formula.
assert.equal(find(teaching, "Now expand $(x+4)(x+6)$ and simplify."), null);
assert.equal(find(teaching, "Something else."), null);
assert.equal(locate([t("Take "), f("x^2+5x+6"), t(".")], "5x+6."), null, "starts inside a formula");
assert.equal(locate([t("Take "), f("x^2+5x+6"), t(".")], "Take x^2"), null, "stops inside a formula");
assert.equal(find(teaching, "   "), null);

console.log("taskLine: every case passed");

// The line beside a task in a reply skipped out of view: its words have no boxes then, and the line stays.
{
  const { ruleAction } = await import("../src/taskLine.ts");
  assert.equal(ruleAction(3, false), "draw");
  assert.equal(ruleAction(0, false), "clear", "words not on the page: no line");
  assert.equal(ruleAction(0, true), "keep", "a skipped reply keeps its line for when it is drawn again");
  console.log("task rule: ok");
}
