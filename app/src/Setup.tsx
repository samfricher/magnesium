// The first screen on a new Mac, and whenever something a lesson needs is
// missing: the magnesium program, Claude Code signed in, how lessons are paid
// for, and a study folder. The window installs its own program, and checks
// Claude Code without installing it. It never reads or stores a credential.

import type { ReactNode } from "react";

import type { About, Account, Payment, RuntimeSetup, Setup } from "./types";

/** Whether a lesson can start: the program runs and Claude Code is signed in. */
export function setupReady(setup: Setup | null): boolean {
  if (!setup) return false;
  const runtime = setup.runtime.state === "ready" || setup.runtime.state === "kept";
  return runtime && setup.account.installed && setup.account.signed_in;
}

/** "0.3.0" from "0.3.0+<commit>". */
export function versionNumber(version: string): string {
  return version.split("+")[0] ?? version;
}

/** The window's version and the first characters of its commit. */
export function aboutText(about: About | null): string | null {
  if (!about) return null;
  return about.commit ? `Magnesium ${about.version} · ${about.commit.slice(0, 7)}` : `Magnesium ${about.version} · built from source`;
}

function Step(props: { ok: boolean; title: string; detail: string; children?: ReactNode }) {
  return (
    <div className={props.ok ? "step" : "step warn"}>
      <div className="head">
        <i />
        <b>{props.title}</b>
        <span className="selectable">{props.detail}</span>
      </div>
      {props.children}
    </div>
  );
}

function RuntimeStep(props: { runtime: RuntimeSetup; onReplace: () => void; onKeep: () => void; onCheck: () => void }) {
  const { runtime } = props;
  const installed = runtime.installed;
  const bundled = runtime.bundled;
  const where = installed ? installed.shown_path : "";
  if (runtime.state === "ready" && installed) {
    return <Step ok title="Magnesium" detail={`${versionNumber(installed.version)} · ${where}${runtime.installed_now ? " · installed just now" : ""}`} />;
  }
  if (runtime.state === "different" && installed && bundled) {
    const theirs = installed.version ? `version ${versionNumber(installed.version)}` : "a copy that does not run";
    return (
      <Step ok={false} title="Magnesium" detail={`another copy · ${where}`}>
        <div className="sub selectable">
          This Mac has Magnesium {theirs}. This app came with {versionNumber(bundled.version)}. Replace it? The window, Claude Code and a
          terminal all use the one copy. The newer one upgrades a study folder&apos;s record when it opens the folder, after asking you, and keeps a
          copy of it first.
        </div>
        <div className="row">
          <button className="btn pri" autoFocus onClick={props.onReplace}>
            Replace &#8629;
          </button>
          <button className="btn ghost" onClick={props.onKeep}>
            Keep {installed.version ? versionNumber(installed.version) : "it"}
          </button>
        </div>
      </Step>
    );
  }
  if (runtime.state === "kept" && installed && bundled) {
    return (
      <Step ok title="Magnesium" detail={`${versionNumber(installed.version)}, kept · ${where}`}>
        <div className="sub selectable">{runtime.message}</div>
        <div className="row">
          <button className="btn sm" onClick={props.onReplace}>
            Replace with {versionNumber(bundled.version)}
          </button>
        </div>
      </Step>
    );
  }
  return (
    <Step ok={false} title="Magnesium" detail={runtime.state === "missing" ? "not installed" : "not installed yet"}>
      <div className="sub selectable">{runtime.message}</div>
      <div className="row">
        <button className="btn sm" onClick={props.onCheck}>
          Try again
        </button>
      </div>
    </Step>
  );
}

function AccountStep(props: { account: Account; signingIn: boolean; onSignIn: () => void; onCheck: () => void; onLink: (url: string) => void }) {
  const { account } = props;
  const name = account.name || "Claude Code";
  if (!account.installed) {
    return (
      <Step ok={false} title={name} detail="not found">
        <div className="sub selectable">
          {account.problem} Magnesium teaches through {name}, which needs a paid Claude plan or an Anthropic Console account.
        </div>
        <div className="row">
          {account.install_page && (
            <button className="btn sm" onClick={() => props.onLink(account.install_page!)}>
              Anthropic&apos;s install page &#8599;
            </button>
          )}
          <button className="btn sm" onClick={props.onCheck}>
            Check again
          </button>
        </div>
      </Step>
    );
  }
  const signIn = (
    <div className="row">
      <button className="btn sm" disabled={props.signingIn} onClick={props.onSignIn}>
        {props.signingIn ? "Waiting for the browser…" : "Sign in"}
      </button>
      <button className="btn ghost sm" onClick={props.onCheck}>
        Check again
      </button>
    </div>
  );
  const waiting = props.signingIn ? " Finish signing in on Anthropic's page in your browser. This screen moves on by itself." : "";
  if (!account.signed_in) {
    return (
      <Step ok={false} title={name} detail="signed out">
        <div className="sub selectable">
          {account.problem}
          {waiting}
        </div>
        {signIn}
      </Step>
    );
  }
  const detail = `signed in · pays with ${account.detail ?? "your account"}`;
  if (account.problem) {
    return (
      <Step ok={false} title={name} detail={detail}>
        <div className="sub selectable">
          {account.problem}
          {waiting}
        </div>
        {signIn}
      </Step>
    );
  }
  return <Step ok title={name} detail={detail} />;
}

export function Welcome(props: {
  setup: Setup | null;
  about: About | null;
  recent: string[];
  error: string | null;
  signingIn: boolean;
  /** A folder is open, so this screen was opened from ⌘K. */
  canGoBack: boolean;
  onPick: () => void;
  onNew: () => void;
  onOpen: (path: string) => void;
  onBack: () => void;
  onReplace: () => void;
  onKeep: () => void;
  onCheck: () => void;
  onSignIn: () => void;
  onPayment: (payment: Payment) => void;
  onLink: (url: string) => void;
}) {
  const { setup } = props;
  const ready = setupReady(setup);
  const choosing = setup?.runtime.state === "different";
  const version = aboutText(props.about);

  let heading: JSX.Element;
  if (!setup) {
    heading = (
      <>
        <div className="meta">checking this Mac</div>
        <h2 className="q">One moment</h2>
        <div className="sub">The window looks for Magnesium and Claude Code.</div>
      </>
    );
  } else if (ready && props.canGoBack) {
    heading = (
      <>
        <div className="meta">this Mac</div>
        <h2 className="q">Magnesium and Claude Code</h2>
        <div className="sub">What lessons run on, and how they are paid for.</div>
      </>
    );
  } else if (ready) {
    heading = (
      <>
        <div className="meta">no folder open</div>
        <h2 className="q">Open a study folder</h2>
        <div className="sub">The folder with your course files, or a new one. Your record stays in it.</div>
      </>
    );
  } else {
    heading = (
      <>
        <div className="meta">setting up this Mac</div>
        <h2 className="q">Before the first lesson</h2>
        <div className="sub">Magnesium runs on this Mac and teaches through Claude Code.</div>
      </>
    );
  }

  return (
    <section className="scr">
      <div className="top" data-tauri-drag-region>
        <span>magnesium</span>
      </div>
      <div className="col">
        <div className="card">
          {heading}
          {setup && (
            <div className="steps">
              <RuntimeStep runtime={setup.runtime} onReplace={props.onReplace} onKeep={props.onKeep} onCheck={props.onCheck} />
              <AccountStep
                account={setup.account}
                signingIn={props.signingIn}
                onSignIn={props.onSignIn}
                onCheck={props.onCheck}
                onLink={props.onLink}
              />
              <div className="pay">
                <span className="lbl">Pay for lessons with</span>
                <div className="row">
                  <button className={setup.payment === "plan" ? "btn sm on" : "btn sm"} onClick={() => props.onPayment("plan")}>
                    Your Claude plan
                  </button>
                  <button className={setup.payment === "api_key" ? "btn sm on" : "btn sm"} onClick={() => props.onPayment("api_key")}>
                    An API key, per use
                  </button>
                </div>
              </div>
            </div>
          )}
          <div className="row">
            {props.canGoBack ? (
              <button className="btn pri" autoFocus={!choosing} onClick={props.onBack}>
                Back &#8629;
              </button>
            ) : (
              <>
                <button className="btn pri" autoFocus={ready && !choosing} disabled={!ready} onClick={props.onPick}>
                  Open a folder &#8629;
                </button>
                <button className="btn ghost" disabled={!ready} onClick={props.onNew}>
                  New folder…
                </button>
                {setup && !ready && <span className="lock">Finish the steps above first</span>}
              </>
            )}
          </div>
          {props.error && <div className="note warn selectable">{props.error}</div>}
          {ready && !props.canGoBack && props.recent.length > 0 && (
            <div className="recent">
              {props.recent.slice(0, 5).map((path) => (
                <button key={path} onClick={() => props.onOpen(path)}>
                  {path.split("/").filter(Boolean).pop()}
                  <span>{path}</span>
                </button>
              ))}
            </div>
          )}
          {setup && (
            <div className="aside selectable">
              Setting a folder up also writes two things outside it: Magnesium&apos;s plugin for Claude Code, in ~/Library/Application
              Support/magnesium, and one entry in Claude Code&apos;s settings that names it.
            </div>
          )}
          {version && <div className="aside selectable">{version}</div>}
        </div>
      </div>
    </section>
  );
}
