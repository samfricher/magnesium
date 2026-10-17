import { spawn } from "node:child_process";

const MAGNESIUM_BIN = "__MAGNESIUM_BINARY__";
const MAGNESIUM_VERSION = "__MAGNESIUM_VERSION__";

const ADAPTER_PROTOCOL_VERSION = "2";
const HOOK_TIMEOUT_MS = 5000;
const SESSION_LOOKUP_TIMEOUT_MS = HOOK_TIMEOUT_MS;
const TELEMETRY_TIMEOUT_MS = 1000;
const MAX_OUTPUT_BYTES = 4 * 1024 * 1024;
const SYNTHETIC_PART_PREFIX = "prt_magnesium_";

const HOOK_EVENT_NAMES = {
  "session-start": "SessionStart",
  "user-prompt-submit": "UserPromptSubmit",
  stop: "Stop",
  "post-compact": "PostCompact",
  "session-end": "SessionEnd",
};

const sessions = new Map();
const sessionQueues = new Map();

function magnesiumBinary() {
  if (process.env.MAGNESIUM_BIN) return process.env.MAGNESIUM_BIN;
  if (MAGNESIUM_BIN && MAGNESIUM_BIN !== "__MAGNESIUM_BINARY__") return MAGNESIUM_BIN;
  return "magnesium";
}

async function boundedSdkCall(invoke, timeoutMs) {
  const controller = new AbortController();
  let timer;
  const timeout = new Promise((resolve) => {
    timer = setTimeout(() => {
      resolve(null);
      controller.abort();
    }, timeoutMs);
  });
  try {
    return await Promise.race([
      Promise.resolve().then(() => invoke(controller.signal)),
      timeout,
    ]);
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

async function logFailure(client, operation, detail) {
  try {
    if (client && client.app && typeof client.app.log === "function") {
      await boundedSdkCall(
        (signal) => client.app.log({
          body: {
            service: "magnesium",
            level: "warn",
            message: "Magnesium hook " + operation + " failed: " + detail,
          },
          signal,
        }),
        TELEMETRY_TIMEOUT_MS,
      );
    }
  } catch {
    // Logging must never surface into the host.
  }
}

async function runMagnesiumHook(client, operation, payload) {
  const eventName = HOOK_EVENT_NAMES[operation];
  if (!eventName) return null;
  const sessionID =
    payload && typeof payload.session_id === "string" ? payload.session_id : "";
  // One promise chain per host session preserves operation order for that
  // session (capture before stop) while unrelated sessions proceed.
  return enqueueSessionWork(sessionID, () =>
    spawnMagnesiumHook(client, operation, eventName, payload),
  );
}

function enqueueSessionWork(sessionID, work) {
  const tail = sessionQueues.get(sessionID) || Promise.resolve();
  const next = tail.then(work, work);
  // Store `next` itself: the cleanup below compares against the stored value,
  // so a finally-derived wrapper would never match and entries would leak.
  sessionQueues.set(sessionID, next);
  next.finally(() => {
    if (sessionQueues.get(sessionID) === next) sessionQueues.delete(sessionID);
  });
  return next;
}

// Asynchronous, bounded execution of the native hook. Slow native work must
// not freeze the plugin event loop: timers, cancellation, and other sessions
// keep running while one hook waits. The caller awaits the returned promise
// for ordering (capture before inference) without blocking the loop.
function spawnMagnesiumHook(client, operation, eventName, payload) {
  return new Promise((resolve) => {
    let finished = false;
    let child = null;
    const done = (result, failure) => {
      if (finished) return;
      finished = true;
      try {
        if (child) child.kill("SIGKILL");
      } catch {
        // The child already exited; nothing to stop.
      }
      if (failure) {
        // Fire-and-forget: an unsettled logging request must never hold the
        // hook (and its session queue) open. The promise is rejection-safe by
        // construction, so no handler is needed.
        try {
          logFailure(client, operation, failure);
        } catch {
          // Logging must never surface into the host.
        }
      }
      resolve(result);
    };
    const args = [
      "_internal",
      "hook",
      operation,
      "--plugin-version",
      MAGNESIUM_VERSION,
      "--adapter-protocol-version",
      ADAPTER_PROTOCOL_VERSION,
    ];
    try {
      child = spawn(magnesiumBinary(), args, {
        stdio: ["pipe", "pipe", "ignore"],
        windowsHide: true,
      });
    } catch {
      done(null, "spawn error");
      return;
    }
    const chunks = [];
    let bytes = 0;
    const timer = setTimeout(() => {
      done(null, "timeout");
    }, HOOK_TIMEOUT_MS);
    child.stdout.on("data", (data) => {
      bytes += data.length;
      if (bytes > MAX_OUTPUT_BYTES) {
        clearTimeout(timer);
        done(null, "output limit");
        return;
      }
      chunks.push(data);
    });
    child.on("error", () => {
      clearTimeout(timer);
      done(null, "spawn error");
    });
    child.on("close", async (code) => {
      clearTimeout(timer);
      if (code !== 0) {
        await done(null, "exit " + code);
        return;
      }
      const text = Buffer.concat(chunks).toString("utf8");
      if (text.trim() === "") {
        await done(null);
        return;
      }
      try {
        await done(JSON.parse(text));
      } catch {
        await done(null, "invalid output");
      }
    });
    try {
      child.stdin.write(JSON.stringify({ ...payload, hook_event_name: eventName }));
      child.stdin.end();
    } catch {
      clearTimeout(timer);
      done(null, "spawn error");
    }
    child.stdin.on("error", () => {
      // A racing child exit surfaces EPIPE asynchronously outside the sync
      // write above; the close handler already decides the outcome.
    });
  });
}

function additionalContext(response) {
  if (!response || !response.hookSpecificOutput) return "";
  const context = response.hookSpecificOutput.additionalContext;
  return typeof context === "string" ? context : "";
}

function sessionIdentity(info, properties) {
  if (info && typeof info.id === "string" && info.id.length > 0) return info.id;
  if (properties && typeof properties.sessionID === "string" && properties.sessionID.length > 0) {
    return properties.sessionID;
  }
  return "";
}

// Subagent sessions carry the model-authored task prompt as their user
// message, so they are never trusted learner input. Top-level sessions have no
// parentID. When the session kind cannot be verified, skip capture.
function isChildSession(info) {
  if (!info || typeof info !== "object") return true;
  return typeof info.parentID === "string" && info.parentID.length > 0;
}

// Tri-state lookup so a transient verification failure skips one turn without
// permanently disabling capture for the session. The OpenCode plugin client
// returns error objects instead of throwing, so the response must be a clean
// session object with the exact requested id: anything less (an `{error:
// ...}` result, a missing/null payload, or an id mismatch) is unverified and
// must never authorize capture.
async function sessionKind(client, sessionID, tracked) {
  if (tracked && typeof tracked.child === "boolean") {
    return tracked.child ? "child" : "top";
  }
  try {
    if (!client || !client.session || typeof client.session.get !== "function") {
      return "unknown";
    }
    const result = await boundedSdkCall(
      (signal) => client.session.get({ path: { id: sessionID }, signal }),
      SESSION_LOOKUP_TIMEOUT_MS,
    );
    if (!result || typeof result !== "object" || result.error != null) {
      return "unknown";
    }
    const info =
      result.data && typeof result.data === "object" ? result.data : result;
    if (!info || typeof info !== "object") return "unknown";
    if (typeof info.id !== "string" || info.id.length === 0 || info.id !== sessionID) {
      return "unknown";
    }
    return isChildSession(info) ? "child" : "top";
  } catch {
    return "unknown";
  }
}

export const Magnesium = async ({ directory, client }) => {
  const cwd =
    typeof directory === "string" && directory.length > 0 ? directory : process.cwd();
  return {
    "chat.message": async (input, output) => {
      try {
        const sessionID = input && typeof input.sessionID === "string" ? input.sessionID : "";
        const message = output && output.message;
        if (!sessionID || !message || typeof message.id !== "string" || message.id.length === 0) {
          return;
        }
        if (!output || !Array.isArray(output.parts)) return;
        let prompt = "";
        for (const part of output.parts) {
          if (!part || part.type !== "text" || part.synthetic || part.ignored) continue;
          if (typeof part.text !== "string") continue;
          prompt = prompt.length === 0 ? part.text : prompt + "\n" + part.text;
        }
        if (prompt.length === 0) return;
        const tracked = sessions.get(sessionID);
        const kind = await sessionKind(client, sessionID, tracked);
        if (kind !== "top") {
          if (kind === "child") {
            sessions.set(sessionID, { ...(tracked || {}), child: true });
          }
          return;
        }
        const turn = message.id;
        const startContext =
          tracked && typeof tracked.startContext === "string" ? tracked.startContext : "";
        const response = await runMagnesiumHook(client, "user-prompt-submit", {
          session_id: sessionID,
          cwd,
          host_kind: "opencode",
          turn_id: turn,
          prompt,
        });
        sessions.set(sessionID, { ...(tracked || {}), child: false, turn, stopped: false, startContext: "" });
        const context = additionalContext(response);
        const text = [startContext, context]
          .filter((value) => value.length > 0)
          .join("\n\n");
        if (text.length > 0) {
          output.parts.push({
            id: SYNTHETIC_PART_PREFIX + turn,
            sessionID,
            messageID: turn,
            type: "text",
            text,
            synthetic: true,
          });
        }
      } catch {
        void logFailure(client, "chat.message", "hook error");
      }
    },
    event: async ({ event }) => {
      try {
        if (!event || typeof event.type !== "string") return;
        const properties =
          event.properties && typeof event.properties === "object" ? event.properties : {};
        if (event.type === "session.created") {
          const sessionID = sessionIdentity(properties.info, properties);
          if (!sessionID) return;
          if (isChildSession(properties.info)) {
            // Trusting creation to *exclude* is fail-closed; it never
            // authorizes capture. Top-level verification stays with the first
            // chat.message lookup so a missing parentID can never mint trust.
            sessions.set(sessionID, { child: true });
            return;
          }
          const response = await runMagnesiumHook(client, "session-start", {
            session_id: sessionID,
            cwd,
            host_kind: "opencode",
            source: "startup",
          });
          const context = additionalContext(response);
          if (context) {
            const tracked = sessions.get(sessionID) || {};
            sessions.set(sessionID, { ...tracked, startContext: context });
          }
          return;
        }
        if (event.type === "session.idle" || event.type === "session.error") {
          const sessionID =
            typeof properties.sessionID === "string" ? properties.sessionID : "";
          if (!sessionID) return;
          const tracked = sessions.get(sessionID);
          if (tracked && tracked.child) {
            sessions.delete(sessionID);
            return;
          }
          if (!tracked || typeof tracked.turn !== "string" || tracked.turn.length === 0) {
            sessions.delete(sessionID);
            return;
          }
          if (tracked.stopped) {
            sessions.delete(sessionID);
            return;
          }
          tracked.stopped = true;
          await runMagnesiumHook(client, "stop", {
            session_id: sessionID,
            cwd,
            host_kind: "opencode",
            turn_id: tracked.turn,
            stop_hook_active: true,
          });
          sessions.delete(sessionID);
          return;
        }
        if (event.type === "session.compacted") {
          const sessionID =
            typeof properties.sessionID === "string" ? properties.sessionID : "";
          if (!sessionID) return;
          const tracked = sessions.get(sessionID);
          const kind = await sessionKind(client, sessionID, tracked);
          if (kind !== "top") {
            if (kind === "child") {
              sessions.set(sessionID, { ...(tracked || {}), child: true });
            }
            return;
          }
          await runMagnesiumHook(client, "post-compact", {
            session_id: sessionID,
            cwd,
            host_kind: "opencode",
            trigger: "auto",
          });
          return;
        }
        if (event.type === "session.deleted") {
          const sessionID = sessionIdentity(properties.info, properties);
          if (!sessionID) return;
          const tracked = sessions.get(sessionID);
          const kind = await sessionKind(client, sessionID, tracked);
          if (kind !== "top") {
            sessions.delete(sessionID);
            return;
          }
          await runMagnesiumHook(client, "session-end", {
            session_id: sessionID,
            cwd,
            host_kind: "opencode",
            reason: "other",
          });
          sessions.delete(sessionID);
        }
      } catch {
        void logFailure(client, "event", "hook error");
      }
    },
  };
};
