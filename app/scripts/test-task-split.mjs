// Checks for splitAtTask, which starts a new paragraph where a task's words
// begin so the line beside the task takes in nothing else. Run by
// scripts/check.sh; node runs the TypeScript file as it is.
//
//   node scripts/test-task-split.mjs

import assert from "node:assert/strict";
import { splitAtTask } from "../src/taskSplit.ts";

const T = "Now your turn: factorise x² + 7x + 12.";
const split = (text, task = T) => splitAtTask(text, task);
const same = (text, why, task = T) => assert.equal(split(text, task), text, why);

// The task ends a teaching paragraph: it becomes a paragraph of its own.
assert.equal(split(`Look for two numbers, here 2 and 3. ${T}`), `Look for two numbers, here 2 and 3.\n\n${T}`);
assert.equal(split(`Teaching.\n${T}\n`), `Teaching.\n\n${T}\n`, "on its own line in the paragraph");
assert.equal(split(`Right. ${T}\n\nA) one\n`), `Right.\n\n${T}\n\nA) one\n`, "options follow in their own paragraph");
assert.equal(split(`- one\n- two\n\nTeaching. ${T}`), `- one\n- two\n\nTeaching.\n\n${T}`, "a list in an earlier paragraph");
assert.equal(split(`Closed **bold** and *italics*, \\(x\\) and [a link](notes.md). ${T}`), `Closed **bold** and *italics*, \\(x\\) and [a link](notes.md).\n\n${T}`, "markup closed before the task");
assert.equal(split(`Use snake_case names. ${T}`), `Use snake_case names.\n\n${T}`, "an underscore inside a word is no italics");
assert.equal(split(`Now: **${T}**`, `**${T}**`), `Now:\n\n**${T}**`, "a task that is itself bold");

// Nothing to do, or words the reply does not hold exactly.
same(`Teaching.\n\n${T}`, "already a paragraph");
same(T, "the whole reply");
same("Something else entirely.", "not in the reply");
same(`Teaching. ${T.replace("²", "^2")}`, "near the reply's words but not them");
same(`Teaching. ${T} Then tell me the numbers.`, "not at the end of its paragraph");

// Blocks a blank line would break.
same(`- Teaching. ${T}`, "a list item");
same(`1. Teaching. ${T}`, "a numbered item");
same(`> Teaching. ${T}`, "a quote");
same(`| a | ${T}`, "a table");
same(`# Teaching ${T}`, "a heading");
same("```\nTeaching. " + T, "a code block");
same(`$$\nx\n${T}`, "a display formula");

// The task's words start inside inline markup.
same(`Teaching **and ${T}**`, "bold", `${T}**`);
same(`Teaching **and ${T}`, "bold left open");
same(`Teaching __and ${T}`, "bold with underscores");
same(`Teaching *and ${T}`, "italics");
same(`Teaching _and ${T}`, "italics with underscores");
same(`Teaching ***and ${T}`, "bold italics");
same(`See [the notes and ${T}`, "a link's words");
same(`See [notes](https://example.com/${T}`, "a link's address");
same(`Teaching \\(x + ${T}`, "a \\( \\) formula");
same(`Teaching $x + ${T}`, "a $ $ formula");
same(`Teaching \`x + ${T}`, "inline code");

console.log("splitAtTask: every case passed");
