// This Mac, as a document: what is set up, in three plain lines, and what is
// left to do. On the very first open it is the only document, with the one
// thing to do next: open or make a study folder.

import type { About, Account, Payment, RuntimeSetup, Setup } from "../types";

export function setupReady(setup: Setup | null): boolean {
  // "too_old" is not ready: no lesson starts until the program is replaced.
  if (!setup) return false;
  const runtime = setup.runtime.state === "ready" || setup.runtime.state === "kept";
  return runtime && setup.account.installed && setup.account.signed_in;
}

const versionNumber = (version: string) => version.split("+")[0] ?? version;

type Line = { ok: boolean; what: string; detail: string; note?: string | null; actions?: React.ReactNode };

function runtimeLine(runtime: RuntimeSetup, act: SetupProps): Line {
  const installed = runtime.installed;
  const bundled = runtime.bundled;
  const where = installed?.shown_path ?? "";
  if (runtime.state === "ready" && installed) {
    return { ok: true, what: "Magnesium", detail: `${versionNumber(installed.version)}, at ${where}${runtime.installed_now ? ", installed just now" : ""}` };
  }
  if (runtime.state === "kept" && installed) {
    return {
      ok: true,
      what: "Magnesium",
      detail: `${versionNumber(installed.version)}, kept, at ${where}`,
      note: runtime.message,
      actions: bundled ? <button onClick={act.onReplace}>Replace with {versionNumber(bundled.version)}</button> : null,
    };
  }
  if (runtime.state === "different" && installed && bundled) {
    return {
      ok: false,
      what: "Magnesium",
      detail: `another version, ${versionNumber(installed.version) || "one that does not run"}, at ${where}`,
      note: `This app came with ${versionNumber(bundled.version)}. The window, Claude Code and a terminal all use one copy. A newer one upgrades a folder's record when it opens it, after asking, and keeps a copy first.`,
      actions: (
        <>
          <button className="primary" onClick={act.onReplace}>
            Replace it
          </button>
          <button onClick={act.onKeep}>Keep {versionNumber(installed.version) || "it"}</button>
        </>
      ),
    };
  }
  if (runtime.state === "too_old") {
    return {
      ok: false,
      what: "Magnesium",
      detail: installed ? `${versionNumber(installed.version)}, too old, at ${where}` : "too old",
      note: runtime.message,
      actions: (
        <>
          {bundled && (
            <button className="primary" onClick={act.onReplace}>
              Replace with {versionNumber(bundled.version)}
            </button>
          )}
          <button onClick={act.onCheck}>Check again</button>
        </>
      ),
    };
  }
  return {
    ok: false,
    what: "Magnesium",
    detail: runtime.state === "missing" ? "not installed" : "does not run",
    note: runtime.message,
    actions: <button onClick={act.onCheck}>Try again</button>,
  };
}

function accountLine(account: Account, act: SetupProps): Line {
  const name = account.name || "Claude Code";
  if (!account.installed) {
    return {
      ok: false,
      what: name,
      detail: "not found on this Mac",
      note: `${account.problem ?? ""} Magnesium teaches through ${name}, which needs a paid Claude plan or an Anthropic Console account.`.trim(),
      actions: (
        <>
          {account.install_page && <button onClick={() => act.onLink(account.install_page!)}>Anthropic's install page</button>}
          <button onClick={act.onCheck}>Check again</button>
        </>
      ),
    };
  }
  const signIn = (
    <>
      <button className="primary" disabled={act.signingIn} onClick={act.onSignIn}>
        {act.signingIn ? "Waiting for the browser…" : "Sign in"}
      </button>
      <button onClick={act.onCheck}>Check again</button>
    </>
  );
  const waiting = act.signingIn ? " Finish signing in on Anthropic's page in your browser; this moves on by itself." : "";
  if (!account.signed_in) return { ok: false, what: name, detail: "signed out", note: `${account.problem ?? ""}${waiting}`.trim() || null, actions: signIn };
  const detail = `signed in, paid with ${account.detail ?? "your account"}`;
  if (account.problem) return { ok: false, what: name, detail, note: `${account.problem}${waiting}`, actions: signIn };
  return { ok: true, what: name, detail };
}

export type SetupProps = {
  setup: Setup | null;
  about: About | null;
  /** No study folder is open: the first open, or every folder was removed. */
  noFolder: boolean;
  signingIn: boolean;
  error: string | null;
  onReplace: () => void;
  onKeep: () => void;
  onCheck: () => void;
  onSignIn: () => void;
  onPayment: (payment: Payment) => void;
  onLink: (url: string) => void;
  onOpenFolder: () => void;
  onNewFolder: () => void;
  onFindFolders: () => void;
  finding: boolean;
  findNote: string | null;
};

export function SetupDoc(props: SetupProps) {
  const { setup } = props;
  const ready = setupReady(setup);
  const lines = setup ? [runtimeLine(setup.runtime, props), accountLine(setup.account, props)] : [];
  return (
    <article className="setup" data-doc="setup">
      <h1 className="doc-title">{!setup ? "One moment" : ready ? (props.noFolder ? "Magnesium" : "This Mac") : "Before your first lesson"}</h1>
      <p className="lead">
        {!setup
          ? "Looking for Magnesium and Claude Code on this Mac."
          : "Magnesium works in a study folder: a folder with your course material in it. Your plan, your lessons and the record of your answers are kept there, on this Mac."}
      </p>
      {setup && (
        <ul className="facts" data-role="setup-facts">
          {lines.map((line) => (
            <li key={line.what} data-ok={line.ok || undefined}>
              <span className="fact-mark" aria-hidden="true">
                {line.ok ? "✓" : "•"}
              </span>
              <span className="fact">
                <span className="fact-what">{line.what}</span> <span className="fact-detail">{line.detail}</span>
                {line.note && <span className="fact-note">{line.note}</span>}
                {line.actions && <span className="fact-actions">{line.actions}</span>}
              </span>
            </li>
          ))}
          <li data-ok>
            <span className="fact-mark" aria-hidden="true">
              ✓
            </span>
            <span className="fact">
              <span className="fact-what">Lessons are paid with</span>{" "}
              <span className="fact-choices" role="radiogroup" aria-label="Lessons are paid with">
                {(["plan", "api_key"] as Payment[]).map((choice) => (
                  <button
                    key={choice}
                    role="radio"
                    aria-checked={setup.payment === choice}
                    className={setup.payment === choice ? "chosen" : undefined}
                    onClick={() => props.onPayment(choice)}
                  >
                    {choice === "plan" ? "your Claude plan" : "an API key"}
                  </button>
                ))}
              </span>
            </span>
          </li>
        </ul>
      )}
      {props.error && <p className="notice">{props.error}</p>}
      {ready && props.noFolder && (
        <ul className="actions" data-role="folder-actions">
          <li>
            <button className="primary" data-drive="start" onClick={props.onOpenFolder} autoFocus>
              Open a study folder <kbd>⌘O</kbd>
            </button>
          </li>
          <li>
            <button onClick={props.onNewFolder}>
              New study folder <kbd>⌘N</kbd>
            </button>
          </li>
          <li>
            <button onClick={props.onFindFolders} disabled={props.finding}>
              {props.finding ? "Looking…" : "Find study folders on this Mac"}
            </button>
            {props.findNote && <span className="quiet"> {props.findNote}</span>}
          </li>
        </ul>
      )}
      {props.about && (
        <p className="doc-path foot">
          Magnesium {versionNumber(props.about.version)}
          {props.about.commit ? `, built from ${props.about.commit.slice(0, 12)}` : ""}
        </p>
      )}
    </article>
  );
}
