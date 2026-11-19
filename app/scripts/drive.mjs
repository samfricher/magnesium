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
