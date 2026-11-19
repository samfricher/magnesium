import assert from "node:assert/strict";
import { registerHooks } from "node:module";

registerHooks({
  resolve(name, context, next) {
    try {
      return next(name, context);
    } catch (error) {
      if (name.startsWith(".") && !name.endsWith(".ts")) return next(`${name}.ts`, context);
      throw error;
    }
  },
});

const { quotable } = await import("../src/selection.ts");
const { splitQuote, withQuote } = await import("../src/lesson.ts");

assert.equal(quotable("  a   b\n c "), "a b c");

const plain = quotable("word ".repeat(150));
assert.ok(plain.endsWith("…"));
assert.ok(plain.length <= 601);

const inline = quotable(`${"x ".repeat(285)}$a + b + c + d + e + f + g + h + i + j + k + l$ and more ${"y ".repeat(40)}`);
assert.ok(inline.endsWith("…"));
assert.equal(inline.includes("$"), false, "a cut inside a formula stops before it");

const display = quotable(`${"x ".repeat(284)}$$a + b + c + d + e + f + g + h + i + j + k + l$$ and more ${"y ".repeat(40)}`);
assert.equal(display.includes("$"), false, "a cut inside a display formula stops before it");

const whole = quotable(`${"x ".repeat(200)}$t^*$ ${"y ".repeat(200)}`);
assert.ok(whole.includes("$t^*$"), "a formula before the cut is kept whole");

const price = quotable(`A ticket costs $5 at the door. ${"and then some more words ".repeat(40)}`);
assert.ok(price.startsWith("A ticket costs $5 at the door. and then"), `a dollar amount is not a formula: ${price.slice(0, 60)}`);
assert.ok(price.length > 500);

assert.deepEqual(splitQuote(withQuote("falls because $t^*$ falls", "Why?")), { quote: "falls because $t^*$ falls", text: "Why?" });

console.log("quotes: every case passed");
