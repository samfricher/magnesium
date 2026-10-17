
import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as installedModule from "./magnesium.js";

const { Magnesium } = installedModule;

const root = mkdtempSync(join(tmpdir(), "magnesium-opencode-adapter-"));
const logPath = join(root, "invocations.log");

writeFileSync(
  join(root, "hook-stub.sh"),
  `#!/bin/sh
{
printf 'op=%s\\n' "$3"
cat
printf '\\n---\\n'
} >> "${logPath}"
if [ "$MAGNESIUM_STUB_MODE" = "junk" ]; then printf 'not-json{{{'; exit 0; fi
if [ "$MAGNESIUM_STUB_MODE" = "exit3" ]; then exit 3; fi
if [ -n "$MAGNESIUM_STUB_SLEEP" ]; then sleep "$MAGNESIUM_STUB_SLEEP"; fi
printf '{"hookSpecificOutput":{"additionalContext":"Magnesium note for test"}}'
`,
);
chmodSync(join(root, "hook-stub.sh"), 0o755);

process.env.MAGNESIUM_BIN = join(root, "hook-stub.sh");
delete process.env.MAGNESIUM_STUB_SLEEP;

function resetLog() {
  writeFileSync(logPath, "");
}

function invocations() {
  const raw = readFileSync(logPath, "utf8");
  return raw
    .split("\n---\n")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
    .map((entry) => {
      const lines = entry.split("\n");
      const op = lines[0].startsWith("op=") ? lines[0].slice(3) : "";
      return { op, payload: JSON.parse(lines.slice(1).join("\n")) };
    });
}

function mockClient(lookup) {
  return {
    app: { log: async () => {} },
    session: { get: lookup },
  };
}

function userTurn(id, texts) {
  return {
    sessionID: "placeholder",
    message: { id },
    parts: texts.map((text, index) => ({
      id: `prt_${id}_${index}`,
      sessionID: "placeholder",
      messageID: id,
      type: "text",
      text,
    })),
  };
}

let passed = 0;
const caseFilter = process.argv[2] || "";
async function check(name, run) {
  if (caseFilter && !name.includes(caseFilter)) return;
  resetLog();
  await run();
  passed += 1;
  console.log(`ok ${passed} - ${name}`);
}

const cwd = "/tmp/demo-workspace";

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function settlesWithin(promise, timeoutMs) {
  let timer;
  try {
    return await Promise.race([
      promise.then(() => true),
      new Promise((resolve) => {
        timer = setTimeout(() => resolve(false), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

await check("noisy valid hook appends attributed context without warning", async () => {
  const stub = join(root, "noisy-hook.mjs");
  const previousBinary = process.env.MAGNESIUM_BIN;
  writeFileSync(stub, `#!${process.execPath}
process.stdin.resume();
process.stdin.on("end", () => {
  process.stderr.write(Buffer.alloc(1024 * 1024, "x"), () => {
    process.stdout.end(JSON.stringify({
      hookSpecificOutput: { additionalContext: "Context after finite diagnostics" },
    }));
  });
});
`);
  chmodSync(stub, 0o755);
  process.env.MAGNESIUM_BIN = stub;
  try {
    const warnings = [];
    const hooks = await Magnesium({
      directory: cwd,
      client: {
        app: { log: async (entry) => warnings.push(entry.body) },
        session: { get: async () => ({ data: { id: "ses-noisy" } }) },
      },
    });
    const turn = userTurn("msg-noisy", ["Original learner text"]);
    turn.parts[0].sessionID = "ses-noisy";
    const original = structuredClone(turn.parts[0]);
    await hooks["chat.message"]({ sessionID: "ses-noisy" }, turn);
    assert.deepEqual(warnings, []);
    assert.deepEqual(turn.parts, [original, {
      id: "prt_magnesium_msg-noisy",
      sessionID: "ses-noisy",
      messageID: "msg-noisy",
      type: "text",
      text: "Context after finite diagnostics",
      synthetic: true,
    }]);
  } finally {
    process.env.MAGNESIUM_BIN = previousBinary;
    rmSync(stub, { force: true });
  }
});

await check("tool arguments remain unchanged for Magnesium and other providers", async () => {
  const hooks = await Magnesium({ directory: cwd, client: mockClient(async () => ({})) });
  const expected = {
    sources: '{"path":"lesson.md"}',
    enabled: "true",
    session: '{"operation":"start","goal":"Learn coefficient roles"}',
    observations: "[]",
    memory_edits: "[]",
  };
  for (const tool of ["magnesium_learning_step", "other_learning_step", "context"]) {
    const output = { args: structuredClone(expected) };
    await hooks["tool.execute.before"]?.({ tool }, output);
    assert.deepEqual(output.args, expected, `${tool} arguments must reach their provider unchanged`);
  }
});

await check("stdout overflow is discarded with a fixed warning", async () => {
  const stub = join(root, "overflow-hook.mjs");
  const previousBinary = process.env.MAGNESIUM_BIN;
  writeFileSync(stub, `#!${process.execPath}
process.stdin.resume();
process.stdin.on("end", () => process.stdout.end(JSON.stringify({
  hookSpecificOutput: { additionalContext: "x".repeat(4 * 1024 * 1024) },
})));
`);
  chmodSync(stub, 0o755);
  process.env.MAGNESIUM_BIN = stub;
  try {
    const warnings = [];
    const hooks = await Magnesium({
      directory: cwd,
      client: {
        app: { log: async (entry) => warnings.push(entry.body) },
        session: { get: async () => ({ data: { id: "ses-overflow" } }) },
      },
    });
    const turn = userTurn("msg-overflow", ["Original learner text"]);
    const original = structuredClone(turn.parts);
    await hooks["chat.message"]({ sessionID: "ses-overflow" }, turn);
    assert.deepEqual(turn.parts, original);
    assert.deepEqual(warnings, [{
      service: "magnesium",
      level: "warn",
      message: "Magnesium hook user-prompt-submit failed: output limit",
    }]);
  } finally {
    process.env.MAGNESIUM_BIN = previousBinary;
    rmSync(stub, { force: true });
  }
});

await check("early stdin closure preserves a valid hook response", async () => {
  const stub = join(root, "early-close-hook.mjs");
  const previousBinary = process.env.MAGNESIUM_BIN;
  writeFileSync(stub, `#!${process.execPath}
import { closeSync } from "node:fs";
closeSync(0);
process.stdout.end(JSON.stringify({
  hookSpecificOutput: { additionalContext: "Context after closing stdin" },
}));
`);
  chmodSync(stub, 0o755);
  process.env.MAGNESIUM_BIN = stub;
  try {
    const warnings = [];
    const hooks = await Magnesium({
      directory: cwd,
      client: {
        app: { log: async (entry) => warnings.push(entry.body) },
        session: { get: async () => ({ data: { id: "ses-early-close" } }) },
      },
    });
    const turn = userTurn("msg-early-close", ["x".repeat(8 * 1024 * 1024)]);
    const original = structuredClone(turn.parts[0]);
    await hooks["chat.message"]({ sessionID: "ses-early-close" }, turn);
    assert.deepEqual(turn.parts, [original, {
      id: "prt_magnesium_msg-early-close",
      sessionID: "ses-early-close",
      messageID: "msg-early-close",
      type: "text",
      text: "Context after closing stdin",
      synthetic: true,
    }]);
    assert.deepEqual(warnings, []);
  } finally {
    process.env.MAGNESIUM_BIN = previousBinary;
    rmSync(stub, { force: true });
  }
});

// 1. Verified top-level session captures and receives context.
await check("top-level capture appends context", async () => {
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => ({ data: { id: "ses-top" } })),
  });
  const turn = userTurn("msg-top", ["teach me"]);
  turn.sessionID = "ses-top";
  turn.message.sessionID = "ses-top";
  await hooks["chat.message"]({ sessionID: "ses-top" }, turn);
  const calls = invocations();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].op, "user-prompt-submit");
  assert.equal(calls[0].payload.session_id, "ses-top");
  assert.equal(calls[0].payload.host_kind, "opencode");
  const appended = turn.parts.filter((part) => part.synthetic);
  assert.equal(appended.length, 1);
  assert.match(appended[0].text, /Magnesium note for test/);
});

// 2. SDK error objects never authorize capture.
await check("error-object lookup skips capture", async () => {
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => ({
      error: { name: "NotFoundError", data: { message: "session lookup failed" } },
    })),
  });
  const turn = userTurn("msg-error", ["teach me"]);
  turn.sessionID = "ses-error";
  await hooks["chat.message"]({ sessionID: "ses-error" }, turn);
  assert.deepEqual(invocations(), []);
  assert.equal(turn.parts.length, 1);
});

// 3. A thrown lookup never authorizes capture.
await check("thrown lookup skips capture", async () => {
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => {
      throw new Error("transport down");
    }),
  });
  const turn = userTurn("msg-throw", ["teach me"]);
  turn.sessionID = "ses-throw";
  await hooks["chat.message"]({ sessionID: "ses-throw" }, turn);
  assert.deepEqual(invocations(), []);
  assert.equal(turn.parts.length, 1);
});

// 4. Session info for a different session never authorizes capture.
await check("mismatched session id skips capture", async () => {
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => ({ data: { id: "some-other-session" } })),
  });
  const turn = userTurn("msg-mismatch", ["teach me"]);
  turn.sessionID = "ses-mismatch";
  await hooks["chat.message"]({ sessionID: "ses-mismatch" }, turn);
  assert.deepEqual(invocations(), []);
  assert.equal(turn.parts.length, 1);
});

// 4b. Malformed lookup responses (bare objects, empty data, null data) have
// no verifiable id and must never authorize capture; each turn re-verifies
// because unknown results are never cached as top-level trust.
for (const [name, response] of [
  ["bare object", {}],
  ["empty data", { data: {} }],
  ["null data", { data: null }],
]) {
  await check(`malformed lookup (${name}) skips capture`, async () => {
    let lookups = 0;
    const hooks = await Magnesium({
      directory: cwd,
      client: mockClient(async () => {
        lookups += 1;
        return response;
      }),
    });
    for (const turnId of ["msg-malformed-a", "msg-malformed-b"]) {
      const turn = userTurn(turnId, ["teach me"]);
      turn.sessionID = "ses-malformed";
      await hooks["chat.message"]({ sessionID: "ses-malformed" }, turn);
      assert.equal(turn.parts.length, 1);
    }
    assert.equal(lookups, 2, "every unverified turn must re-run the lookup");
    assert.deepEqual(
      invocations().filter((call) => call.op === "user-prompt-submit"),
      [],
    );
  });
}

// 5. Child (subagent) sessions are never captured, including via events.
await check("child sessions are never captured", async () => {
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => ({ data: { id: "ses-child", parentID: "ses-top" } })),
  });
  await hooks.event({
    event: { type: "session.created", properties: { info: { id: "ses-child", parentID: "ses-top" } } },
  });
  const turn = userTurn("msg-child", ["model-authored task prompt"]);
  turn.sessionID = "ses-child";
  await hooks["chat.message"]({ sessionID: "ses-child" }, turn);
  await hooks.event({
    event: { type: "session.idle", properties: { sessionID: "ses-child" } },
  });
  assert.deepEqual(invocations(), []);
  assert.equal(turn.parts.length, 1);
});

// 6. Synthetic-only messages are never captured and need no lookup.
await check("synthetic-only messages are never captured", async () => {
  let lookedUp = false;
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => {
      lookedUp = true;
      return { data: { id: "ses-synth" } };
    }),
  });
  const turn = userTurn("msg-synth", ["Magnesium note for test"]);
  turn.sessionID = "ses-synth";
  turn.parts[0].synthetic = true;
  await hooks["chat.message"]({ sessionID: "ses-synth" }, turn);
  assert.deepEqual(invocations(), []);
  assert.equal(lookedUp, false);
  assert.equal(turn.parts.length, 1);
});

// 7. Per-session ordering is preserved across concurrent turns.
await check("per-session turns run in order", async () => {
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => ({ data: { id: "ses-order" } })),
  });
  const first = userTurn("msg-first", ["first"]);
  first.sessionID = "ses-order";
  const second = userTurn("msg-second", ["second"]);
  second.sessionID = "ses-order";
  await Promise.all([
    hooks["chat.message"]({ sessionID: "ses-order" }, first),
    hooks["chat.message"]({ sessionID: "ses-order" }, second),
  ]);
  const calls = invocations();
  assert.equal(calls.length, 2);
  assert.equal(calls[0].payload.turn_id, "msg-first");
  assert.equal(calls[1].payload.turn_id, "msg-second");
});

// 8. The event loop stays responsive during a slow hook.
await check("event loop stays responsive during a slow hook", async () => {
  process.env.MAGNESIUM_STUB_SLEEP = "0.3";
  try {
    const hooks = await Magnesium({
      directory: cwd,
      client: mockClient(async () => ({ data: { id: "ses-slow" } })),
    });
    const turn = userTurn("msg-slow", ["a slow turn"]);
    turn.sessionID = "ses-slow";
    let timerAt = 0;
    let resolvedAt = 0;
    const timer = new Promise((resolve) =>
      setTimeout(() => {
        timerAt = Date.now();
        resolve();
      }, 10),
    );
    const pending = (async () => {
      await hooks["chat.message"]({ sessionID: "ses-slow" }, turn);
      resolvedAt = Date.now();
    })();
    await timer;
    await pending;
    assert.ok(timerAt > 0 && resolvedAt > 0);
    assert.ok(
      resolvedAt - timerAt > 100,
      "the turn must have awaited the slow hook instead of racing ahead",
    );
    const appended = turn.parts.filter((part) => part.synthetic);
    assert.equal(appended.length, 1);
  } finally {
    delete process.env.MAGNESIUM_STUB_SLEEP;
  }
});

// 9. A session.created event never authorizes capture on its own: the first
// turn still verifies through the lookup, so a later lookup failure skips.
await check("created event does not authorize capture", async () => {
  let lookups = 0;
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(async () => {
      lookups += 1;
      return { error: { name: "NotFoundError", data: { message: "gone" } } };
    }),
  });
  await hooks.event({
    event: { type: "session.created", properties: { info: { id: "ses-created" } } },
  });
  const turn = userTurn("msg-created", ["teach me"]);
  turn.sessionID = "ses-created";
  await hooks["chat.message"]({ sessionID: "ses-created" }, turn);
  assert.ok(lookups > 0, "the first turn must still verify the session");
  assert.deepEqual(
    invocations().filter((call) => call.op === "user-prompt-submit"),
    [],
  );
  assert.equal(turn.parts.length, 1);
});

// 10. A hook that outlives its timeout is killed and discarded.
await check("hook timeout bounds execution", async () => {
  process.env.MAGNESIUM_STUB_SLEEP = "10";
  try {
    const hooks = await Magnesium({
      directory: cwd,
      client: mockClient(async () => ({ data: { id: "ses-timeout" } })),
    });
    const turn = userTurn("msg-timeout", ["a wedged turn"]);
    turn.sessionID = "ses-timeout";
    const started = Date.now();
    await hooks["chat.message"]({ sessionID: "ses-timeout" }, turn);
    const elapsed = Date.now() - started;
    assert.ok(
      elapsed < 8000,
      `the timeout must bound the hook well before the 10s stub (took ${elapsed}ms)`,
    );
    assert.deepEqual(
      turn.parts.filter((part) => part.synthetic),
      [],
      "a timed-out hook must not inject context",
    );
  } finally {
    delete process.env.MAGNESIUM_STUB_SLEEP;
  }
});

// 11b. An unsettled logging request never holds the hook open.
await check("unsettled logging never blocks the hook", async () => {
  process.env.MAGNESIUM_STUB_MODE = "exit3";
  try {
    const hooks = await Magnesium({
      directory: cwd,
      client: {
        app: { log: () => new Promise(() => {}) },
        session: {
          get: async () => ({ data: { id: "ses-stuck-log" } }),
        },
      },
    });
    const turn = userTurn("msg-stuck-log", ["a turn"]);
    turn.sessionID = "ses-stuck-log";
    const settled = await Promise.race([
      hooks["chat.message"]({ sessionID: "ses-stuck-log" }, turn).then(
        () => "resolved",
      ),
      new Promise((resolve) => setTimeout(() => resolve("timed-out"), 3000)),
    ]);
    assert.equal(settled, "resolved");
    assert.deepEqual(
      turn.parts.filter((part) => part.synthetic),
      [],
      "a failed hook must not inject context",
    );
  } finally {
    delete process.env.MAGNESIUM_STUB_MODE;
  }
});
// 11. Invalid hook output and nonzero exits are discarded silently.
await check("invalid output and nonzero exit are discarded", async () => {
  for (const mode of ["junk", "exit3"]) {
    process.env.MAGNESIUM_STUB_MODE = mode;
    try {
      const hooks = await Magnesium({
        directory: cwd,
        client: mockClient(async () => ({ data: { id: `ses-${mode}` } })),
      });
      const turn = userTurn(`msg-${mode}`, ["a turn"]);
      turn.sessionID = `ses-${mode}`;
      await hooks["chat.message"]({ sessionID: `ses-${mode}` }, turn);
      assert.deepEqual(
        turn.parts.filter((part) => part.synthetic),
        [],
        `mode ${mode} must not inject context`,
      );
    } finally {
      delete process.env.MAGNESIUM_STUB_MODE;
    }
  }
});

await check("production lookup deadline skips late capture and permits recovery", async () => {
  let resolveLookup;
  let lookupSignal;
  let stalledLookups = 0;
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(({ path, signal }) => {
      if (path.id !== "ses-sdk-stalled") return { data: { id: path.id } };
      stalledLookups += 1;
      if (stalledLookups > 1) return { data: { id: path.id } };
      lookupSignal = signal;
      return new Promise((resolve) => { resolveLookup = resolve; });
    }),
  });
  const lateTurn = userTurn("msg-sdk-late", ["An unverified turn"]);
  const original = structuredClone(lateTurn.parts);
  const pending = hooks["chat.message"]({ sessionID: "ses-sdk-stalled" }, lateTurn);
  try {
    const otherTurn = userTurn("msg-sdk-other", ["An unrelated turn"]);
    assert.equal(await settlesWithin(
      hooks["chat.message"]({ sessionID: "ses-sdk-other" }, otherTurn), 2000,
    ), true, "an unrelated session must proceed during stalled verification");
    assert.equal(otherTurn.parts[1].text, "Magnesium note for test");
    assert.deepEqual(invocations().map((call) => call.payload.turn_id), ["msg-sdk-other"]);
    assert.deepEqual(lateTurn.parts, original);
    assert.equal(await settlesWithin(pending, 8000), true,
      "a lookup ignoring abort must settle at the production deadline");
    assert.ok(lookupSignal instanceof AbortSignal);
    assert.equal(lookupSignal.aborted, true);
    assert.deepEqual(lateTurn.parts, original);

    const healthyTurn = userTurn("msg-sdk-healthy", ["A later verified turn"]);
    await hooks["chat.message"]({ sessionID: "ses-sdk-stalled" }, healthyTurn);
    assert.equal(stalledLookups, 2, "unknown session kind must be retried");
    assert.equal(healthyTurn.parts[1].text, "Magnesium note for test");
    resolveLookup({ data: { id: "ses-sdk-stalled" } });
    await wait(0);
    assert.deepEqual(lateTurn.parts, original);
    await hooks.event({
      event: { type: "session.idle", properties: { sessionID: "ses-sdk-stalled" } },
    });
    assert.deepEqual(invocations().map((call) => [call.op, call.payload.turn_id]), [
      ["user-prompt-submit", "msg-sdk-other"],
      ["user-prompt-submit", "msg-sdk-healthy"],
      ["stop", "msg-sdk-healthy"],
    ]);
  } finally {
    resolveLookup?.({ data: { id: "ses-sdk-stalled" } });
    await pending;
  }
});

for (const operation of ["chat.message", "event"]) {
  for (const abortAware of [false, true]) {
    await check(`outer ${operation} exception returns before stalled telemetry, abort-aware=${abortAware}`, async () => {
      let request;
      let stopped;
      const aborted = new Promise((resolve) => { stopped = resolve; });
      const hooks = await Magnesium({
        directory: cwd,
        client: {
          app: { log: (options) => {
            request = options;
            return new Promise((_, reject) => {
              options.signal?.addEventListener("abort", () => {
                stopped();
                if (abortAware) reject(new Error("synthetic request aborted"));
              }, { once: true });
            });
          } },
        },
      });
      const pending = operation === "chat.message"
        ? hooks[operation]({ sessionID: "ses-outer-error" }, {
          message: { id: "msg-outer-error" },
          get parts() { throw new Error("synthetic output getter failure"); },
        })
        : hooks.event({ event: {
          get type() { throw new Error("synthetic event getter failure"); },
        } });
      assert.equal(await settlesWithin(pending, 200), true,
        "the host callback must return independently of telemetry");
      await wait(0);
      assert.deepEqual(request.body, {
        service: "magnesium",
        level: "warn",
        message: `Magnesium hook ${operation} failed: hook error`,
      });
      assert.ok(request.signal instanceof AbortSignal);
      assert.equal(await settlesWithin(aborted, 2000), true,
        "a stalled logger must receive request cancellation");
      assert.equal(request.signal.aborted, true);
      assert.deepEqual(invocations(), []);
    });
  }
}

await check("outer exceptions contain malformed and throwing loggers", async () => {
  const unhandled = [];
  const listener = (reason) => unhandled.push(reason);
  process.on("unhandledRejection", listener);
  try {
    const warnings = [];
    for (const client of [
      undefined,
      {},
      { app: null },
      { app: {} },
      { app: { log: "malformed" } },
      { get app() { throw new Error("synthetic app getter failure"); } },
      { app: { get log() { throw new Error("synthetic log getter failure"); } } },
      { app: { log: () => { throw new Error("synthetic logger throw"); } } },
      { app: { log: () => Promise.reject(new Error("synthetic logger rejection")) } },
      { app: { log: async (request) => warnings.push(request.body) } },
    ]) {
      const hooks = await Magnesium({ directory: cwd, client });
      await hooks["chat.message"]({ sessionID: "ses-malformed-logger" }, {
        message: { id: "msg-malformed-logger" },
        get parts() { throw new Error("synthetic output getter failure"); },
      });
      await hooks.event({ event: {
        get type() { throw new Error("synthetic event getter failure"); },
      } });
    }
    await wait(0);
    assert.deepEqual(warnings, [
      { service: "magnesium", level: "warn", message: "Magnesium hook chat.message failed: hook error" },
      { service: "magnesium", level: "warn", message: "Magnesium hook event failed: hook error" },
    ]);
    assert.deepEqual(unhandled, []);
    assert.deepEqual(invocations(), []);
  } finally {
    process.removeListener("unhandledRejection", listener);
  }
});

await check("production lifecycle lookups time out and observe late rejection", async () => {
  const requests = new Map();
  const unhandled = [];
  const listener = (reason) => unhandled.push(reason);
  process.on("unhandledRejection", listener);
  let healthy = false;
  const hooks = await Magnesium({
    directory: cwd,
    client: mockClient(({ path, signal }) => {
      if (healthy) return { data: { id: path.id } };
      return new Promise((resolve, reject) => {
        requests.set(path.id, { resolve, reject, signal });
        if (path.id === "ses-sdk-abort-aware") {
          signal.addEventListener("abort", () => reject(new Error("synthetic request aborted")), { once: true });
        }
      });
    }),
  });
  const lateTurn = userTurn("msg-sdk-reject", ["An unverified rejected turn"]);
  const original = structuredClone(lateTurn.parts);
  const abortTurn = userTurn("msg-sdk-abort-aware", ["An unverified aborted turn"]);
  const pending = [
    hooks["chat.message"]({ sessionID: "ses-sdk-reject" }, lateTurn),
    hooks["chat.message"]({ sessionID: "ses-sdk-abort-aware" }, abortTurn),
    hooks.event({ event: { type: "session.compacted", properties: { sessionID: "ses-sdk-compact" } } }),
    hooks.event({ event: { type: "session.deleted", properties: { info: { id: "ses-sdk-delete" } } } }),
  ];
  try {
    assert.deepEqual(await Promise.all(pending.map((call) => settlesWithin(call, 8000))), [true, true, true, true]);
    for (const { signal } of requests.values()) {
      assert.ok(signal instanceof AbortSignal);
      assert.equal(signal.aborted, true);
    }
    assert.deepEqual(invocations(), []);
    assert.deepEqual(lateTurn.parts, original);
    assert.equal(abortTurn.parts.length, 1);
    requests.get("ses-sdk-reject").reject(new Error("synthetic late rejection"));
    for (const id of ["ses-sdk-compact", "ses-sdk-delete"]) {
      requests.get(id).resolve({ data: { id } });
    }
    await wait(0);
    assert.deepEqual(unhandled, []);
    assert.deepEqual(invocations(), []);
    assert.deepEqual(lateTurn.parts, original);
    healthy = true;
    await hooks.event({ event: { type: "session.compacted", properties: { sessionID: "ses-sdk-compact" } } });
    await hooks.event({ event: { type: "session.deleted", properties: { info: { id: "ses-sdk-delete" } } } });
    assert.deepEqual(invocations().map((call) => [call.op, call.payload.session_id]), [
      ["post-compact", "ses-sdk-compact"],
      ["session-end", "ses-sdk-delete"],
    ]);
  } finally {
    for (const [id, request] of requests) request.resolve({ data: { id } });
    await Promise.all(pending);
    process.removeListener("unhandledRejection", listener);
  }
});

await check("installed module exposes only the Magnesium plugin", async () => {
  assert.deepEqual(Object.keys(installedModule), ["Magnesium"]);
  const hooks = await Magnesium({ directory: cwd, client: mockClient(async () => ({})) });
  assert.deepEqual(Object.keys(hooks).sort(), ["chat.message", "event"]);
});

const helperSource = readFileSync(new URL("./magnesium.js", import.meta.url), "utf8")
  + "\nexport { boundedSdkCall };\n";
const { boundedSdkCall } = await import(
  "data:text/javascript;base64," + Buffer.from(helperSource).toString("base64"),
);

await check("private SDK helper preserves success and supplies an independent signal", async () => {
  const expected = { data: { id: "ses-helper-success" } };
  let signal;
  assert.equal(await boundedSdkCall((current) => {
    signal = current;
    return expected;
  }, 100), expected);
  assert.ok(signal instanceof AbortSignal);
  assert.equal(signal.aborted, false);
});

await check("private SDK helper contains synchronous exceptions", async () => {
  assert.equal(await boundedSdkCall(() => { throw new Error("synthetic throw"); }, 100), null);
  assert.equal(await boundedSdkCall(() => "healthy after throw", 100), "healthy after throw");
});

await check("private SDK helper contains rejected requests", async () => {
  assert.equal(await boundedSdkCall(() => Promise.reject(new Error("synthetic rejection")), 100), null);
  assert.equal(await boundedSdkCall(() => "healthy after rejection", 100), "healthy after rejection");
});

await check("private SDK helper bounds a request ignoring abort", async () => {
  let signal;
  const pending = boundedSdkCall((current) => {
    signal = current;
    return new Promise(() => {});
  }, 20);
  assert.equal(await settlesWithin(pending, 1000), true);
  assert.equal(await pending, null);
  assert.equal(signal.aborted, true);
  assert.equal(await boundedSdkCall(() => "healthy after ignored abort", 100), "healthy after ignored abort");
});

await check("private SDK helper settles timeout before abort-aware rejection", async () => {
  let aborted = 0;
  assert.equal(await boundedSdkCall((signal) => new Promise((_, reject) => {
    signal.addEventListener("abort", () => {
      aborted += 1;
      reject(new Error("synthetic abort rejection"));
    }, { once: true });
  }), 20), null);
  assert.equal(aborted, 1);
  assert.equal(await boundedSdkCall(() => "healthy after abort", 100), "healthy after abort");
});

await check("private SDK helper discards late success", async () => {
  let resolve;
  let selected = "not settled";
  const pending = boundedSdkCall(() => new Promise((done) => { resolve = done; }), 20)
    .then((result) => { selected = result; return result; });
  assert.equal(await pending, null);
  resolve({ data: { id: "ses-helper-too-late" } });
  await wait(0);
  assert.equal(selected, null);
  assert.equal(await boundedSdkCall(() => "healthy after late success", 100), "healthy after late success");
});

await check("private SDK helper observes late rejection", async () => {
  let reject;
  const unhandled = [];
  const listener = (reason) => unhandled.push(reason);
  process.on("unhandledRejection", listener);
  try {
    assert.equal(await boundedSdkCall(() => new Promise((_, fail) => { reject = fail; }), 20), null);
    reject(new Error("synthetic late helper rejection"));
    await wait(0);
    assert.deepEqual(unhandled, []);
    assert.equal(await boundedSdkCall(() => "healthy after late rejection", 100), "healthy after late rejection");
  } finally {
    process.removeListener("unhandledRejection", listener);
  }
});

await check("private SDK helper retries normally after timeout", async () => {
  assert.equal(await boundedSdkCall(() => new Promise(() => {}), 20), null);
  const expected = { data: { id: "ses-helper-retry" } };
  assert.equal(await boundedSdkCall(() => Promise.resolve(expected), 100), expected);
});

await check("private SDK helper lets concurrent unrelated requests proceed", async () => {
  let slowSignal;
  let healthySignal;
  const slow = boundedSdkCall((signal) => {
    slowSignal = signal;
    return new Promise(() => {});
  }, 20);
  const expected = { data: { id: "ses-helper-unrelated" } };
  assert.equal(await boundedSdkCall((signal) => {
    healthySignal = signal;
    return expected;
  }, 100), expected);
  assert.notEqual(slowSignal, healthySignal);
  assert.equal(healthySignal.aborted, false);
  assert.equal(await slow, null);
  assert.equal(slowSignal.aborted, true);
  assert.equal(healthySignal.aborted, false);
});

await check("private SDK helper clears successful request timers", async () => {
  let signal;
  assert.equal(await boundedSdkCall((current) => {
    signal = current;
    return "complete";
  }, 20), "complete");
  await wait(35);
  assert.equal(signal.aborted, false);
});

console.log(`ALL PLUGIN CASES PASSED (${passed})`);
rmSync(root, { recursive: true, force: true });
