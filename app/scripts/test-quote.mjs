import assert from "node:assert/strict";

const { quotable, splitQuote, withQuote } = await import("../src/lesson.ts");

assert.equal(quotable("  a   b\n c "), "a b c");

const plain = quotable("word ".repeat(150));
assert.ok(plain.endsWith("…"));
assert.ok(plain.length <= 601);

const inline = quotable(`${"x ".repeat(285)}$a + b + c + d + e + f + g + h + i + j + k + l$ and more ${"y ".repeat(40)}`);
assert.ok(inline.endsWith("…"));
assert.equal(inline.includes("$"), false);

const display = quotable(`${"x ".repeat(284)}$$a + b + c + d + e + f + g + h + i + j + k + l$$ and more ${"y ".repeat(40)}`);
assert.equal(display.includes("$"), false);

const whole = quotable(`${"x ".repeat(200)}$t^*$ ${"y ".repeat(200)}`);
assert.ok(whole.includes("$t^*$"));

assert.deepEqual(splitQuote(withQuote("falls because $t^*$ falls", "Why?")), { quote: "falls because $t^*$ falls", text: "Why?" });

console.log("quotes: every case passed");
