// Send one instruction to a window started with MAGNESIUM_APP_DRIVE=<dir>,
// wait for its answer, and print it. No dependencies.
//
//   node scripts/drive.mjs <dir> '{"op":"click","target":"start"}'
//   node scripts/drive.mjs <dir> '{"op":"wait_idle","timeout_ms":120000}'
//
// Exits 0 when the answer says ok, 1 when it does not, 2 when no answer came
// in time (the op's timeout_ms or ms, plus 30 s; 60 s otherwise).
//
// The ops are listed at the top of src/harness.ts. Also importable:
//   import { drive } from "./drive.mjs"; await drive(dir, {op: "text"});

import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function highest(dir) {
  if (!existsSync(dir)) return 0;
  return readdirSync(dir)
    .map((name) => /^(\d+)\.json$/.exec(name))
    .filter(Boolean)
    .reduce((max, match) => Math.max(max, Number(match[1])), 0);
}

/** Send `op` to the window driven from `dir`; resolves to its answer, or {ok:false, error, timed_out:true}. */
export async function drive(dir, op, { timeoutMs } = {}) {
  const inbox = join(dir, "in");
  const outbox = join(dir, "out");
  mkdirSync(inbox, { recursive: true });
  mkdirSync(outbox, { recursive: true });
  const n = Math.max(highest(inbox), highest(outbox)) + 1;
  const partial = join(inbox, `${n}.json.partial`);
  writeFileSync(partial, JSON.stringify(op));
  renameSync(partial, join(inbox, `${n}.json`));
  const limit = timeoutMs ?? Number(op.timeout_ms ?? op.ms ?? 30_000) + 30_000;
  const answer = join(outbox, `${n}.json`);
  const start = Date.now();
  while (Date.now() - start < limit) {
    if (existsSync(answer)) {
      try {
        return JSON.parse(readFileSync(answer, "utf8"));
      } catch {
        // Not whole yet.
      }
    }
    await sleep(50);
  }
  return { ok: false, error: `no answer to ${op.op} within ${limit} ms`, timed_out: true };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [dir, text] = process.argv.slice(2);
  if (!dir || !text) {
    console.error("usage: node scripts/drive.mjs <dir> '<op json>'");
    process.exit(64);
  }
  const op = JSON.parse(text);
  const result = await drive(dir, op);
  console.log(JSON.stringify(result, null, 2));
  process.exit(result.timed_out ? 2 : result.ok ? 0 : 1);
}
