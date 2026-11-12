// What a lesson needs on this Mac: the magnesium program, Claude Code signed
// in, and a way to pay for lessons. It is the first screen on a new Mac, and
// it comes back whenever one of them is missing. The window installs its own
// program, and checks Claude Code without installing it. It never reads or
// stores a credential.

import type { ReactNode } from "react";

import { Icon } from "../ui/icons";
import { TopBar } from "../ui/bits";
import type { About, Account, Payment, RuntimeSetup, Setup as SetupData } from "../types";

/** Whether a lesson can start: the program runs and Claude Code is signed in. */
export function setupReady(setup: SetupData | null): boolean {
  // "too_old" is not ready: no lesson starts until the program is replaced.
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
    <div className={props.ok ? "step ok" : "step"}>
      <span className="step-mark" aria-hidden="true">
        <Icon name={props.ok ? "check" : "warn"} size={props.ok ? 13 : 14} />
      </span>
      <div className="step-body">
        <div className="step-head">
          <b>{props.title}</b>
          <span className="selectable">{props.detail}</span>
        </div>
        {props.children}
      </div>
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
        <p className="selectable">
          This Mac has Magnesium {theirs}. This app came with {versionNumber(bundled.version)}. Replace it? The window, Claude Code and a terminal
          all use the one copy. The newer one upgrades a study folder&apos;s record when it opens the folder, after asking you, and keeps a copy of
          it first.
        </p>
        <div className="row">
          <button className="btn primary" autoFocus title="Replace (Return)" onClick={props.onReplace}>
            Replace
          </button>
          <button className="btn ghost" onClick={props.onKeep}>
            Keep {installed.version ? versionNumber(installed.version) : "it"}
          </button>
        </div>
      </Step>
    );
  }
  if (runtime.state === "too_old" && installed) {
    // Older than the window needs: it cannot be kept, so the only way on is a
    // newer copy, the app's own when it carries one.
    return (
      <Step ok={false} title="Magnesium" detail={`${versionNumber(installed.version)}, too old · ${where}`}>
        <p className="selectable">{runtime.message}</p>
        <div className="row">
          {bundled ? (
            <button className="btn primary" autoFocus title="Replace (Return)" onClick={props.onReplace}>
              Replace with {versionNumber(bundled.version)}
            </button>
          ) : (
            <button className="btn small" onClick={props.onCheck}>
              Check again
            </button>
          )}
        </div>
      </Step>
    );
  }
  if (runtime.state === "kept" && installed && bundled) {
    return (
      <Step ok title="Magnesium" detail={`${versionNumber(installed.version)}, kept · ${where}`}>
        <p className="selectable">{runtime.message}</p>
        <div className="row">
          <button className="btn small" onClick={props.onReplace}>
            Replace with {versionNumber(bundled.version)}
          </button>
        </div>
      </Step>
    );
  }
  return (
    <Step
      ok={false}
      title="Magnesium"
      detail={runtime.state === "missing" ? "not installed" : installed ? `${versionNumber(installed.version) || "does not run"} · ${where}` : "not installed yet"}
    >
      <p className="selectable">{runtime.message}</p>
      <div className="row">
        <button className="btn small" onClick={props.onCheck}>
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
        <p className="selectable">
          {account.problem} Magnesium teaches through {name}, which needs a paid Claude plan or an Anthropic Console account.
        </p>
        <div className="row">
          {account.install_page && (
            <button className="btn small" onClick={() => props.onLink(account.install_page!)}>
              Anthropic&apos;s install page <Icon name="external" size={14} />
            </button>
          )}
          <button className="btn ghost small" onClick={props.onCheck}>
            Check again
          </button>
        </div>
      </Step>
    );
  }
  const signIn = (
    <div className="row">
      <button className="btn small" disabled={props.signingIn} onClick={props.onSignIn}>
        {props.signingIn ? "Waiting for the browser…" : "Sign in"}
      </button>
      <button className="btn ghost small" onClick={props.onCheck}>
        Check again
      </button>
    </div>
  );
  const waiting = props.signingIn ? " Finish signing in on Anthropic's page in your browser. This screen moves on by itself." : "";
  if (!account.signed_in) {
    return (
      <Step ok={false} title={name} detail="signed out">
        <p className="selectable">
          {account.problem}
          {waiting}
        </p>
        {signIn}
      </Step>
    );
  }
  const detail = `signed in · pays with ${account.detail ?? "your account"}`;
  if (account.problem) {
    return (
      <Step ok={false} title={name} detail={detail}>
        <p className="selectable">
          {account.problem}
          {waiting}
        </p>
        {signIn}
      </Step>
    );
  }
  return <Step ok title={name} detail={detail} />;
}

export function Setup(props: {
  setup: SetupData | null;
  about: About | null;
  error: string | null;
  signingIn: boolean;
  /** The library is behind this screen, so there is somewhere to go back to. */
  canGoBack: boolean;
  onBack: () => void;
  onReplace: () => void;
  onKeep: () => void;
  onCheck: () => void;
  onSignIn: () => void;
  onPayment: (payment: Payment) => void;
  onLink: (url: string) => void;
  /** The choice of what runs lessons, or null when the core cannot say. */
  engine: ReactNode;
}) {
  const { setup } = props;
  const ready = setupReady(setup);
  const choosing = setup?.runtime.state === "different" || setup?.runtime.state === "too_old";
  const version = aboutText(props.about);

  let title = "Before your first lesson";
  let sub = "Magnesium runs on this Mac and teaches through Claude Code.";
  if (!setup) {
    title = "One moment";
    sub = "Looking for Magnesium and Claude Code on this Mac.";
  } else if (setup.runtime.state === "too_old") {
    title = "Magnesium needs updating";
    sub = "The program on this Mac is older than this window needs.";
  } else if (ready) {
    title = "This Mac is ready";
    sub = "What lessons run on, and how they are paid for.";
  }

  return (
    <section className="screen setup" data-screen="setup">
      <TopBar
        left={
          props.canGoBack ? (
            <button className="crumb" onClick={props.onBack} title="Back (Esc)">
              <Icon name="back" size={16} />
              Library
            </button>
          ) : undefined
        }
      />
      <div className="center">
        <div className="setup-col">
          <span className="mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <h1>{title}</h1>
          <p className="setup-sub">{sub}</p>
          {setup && (
            <div className="steps">
              <RuntimeStep runtime={setup.runtime} onReplace={props.onReplace} onKeep={props.onKeep} onCheck={props.onCheck} />
              <AccountStep account={setup.account} signingIn={props.signingIn} onSignIn={props.onSignIn} onCheck={props.onCheck} onLink={props.onLink} />
              <div className="step plain">
                <span className="step-mark" aria-hidden="true" />
                <div className="step-body">
                  <div className="step-head">
                    <b>Pay for lessons with</b>
                  </div>
                  <div className="segmented" role="radiogroup" aria-label="Pay for lessons with">
                    <button role="radio" aria-checked={setup.payment === "plan"} className={setup.payment === "plan" ? "on" : undefined} onClick={() => props.onPayment("plan")}>
                      Your Claude plan
                    </button>
                    <button
                      role="radio"
                      aria-checked={setup.payment === "api_key"}
                      className={setup.payment === "api_key" ? "on" : undefined}
                      onClick={() => props.onPayment("api_key")}
                    >
                      An API key, per use
                    </button>
                  </div>
                </div>
              </div>
              {props.engine && (
                <div className="step plain">
                  <span className="step-mark" aria-hidden="true" />
                  <div className="step-body">
                    <div className="step-head">
                      <b>Lessons run on</b>
                    </div>
                    {props.engine}
                  </div>
                </div>
              )}
            </div>
          )}
          {props.error && <div className="banner bad selectable">{props.error}</div>}
          <div className="row center">
            <button className="btn primary" autoFocus={ready && !choosing} disabled={!ready} title="Done (Return)" onClick={props.onBack} data-role="setup-done">
              {props.canGoBack ? "Done" : "Go to your library"}
            </button>
            {setup && !ready && <span className="aside">Finish the steps above first.</span>}
          </div>
          {setup && (
            <p className="aside selectable">
              Setting a study folder up also writes two things outside it: Magnesium&apos;s plugin for Claude Code, in ~/Library/Application
              Support/magnesium, and one entry in Claude Code&apos;s settings that names it.
            </p>
          )}
          {version && <p className="aside version selectable">{version}</p>}
        </div>
      </div>
    </section>
  );
}
