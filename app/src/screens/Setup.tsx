// What a lesson needs on this Mac: the magnesium program, Claude Code signed
// in, and a way to pay for lessons. It is the first screen on a new Mac, and
// it comes back whenever one of them is missing. The window installs its own
// program, and checks Claude Code without installing it. It never reads or
// stores a credential.

import type { ReactNode } from "react";

import { Icon } from "../ui/icons";
import { Choice, Fault, PaletteKey, TopBar, useReturnForPrimary } from "../ui/bits";
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
  return about.commit ? `Magnesium ${about.version} (${about.commit.slice(0, 7)})` : `Magnesium ${about.version}, built from source`;
}

/**
 * A step is done (an ink tick), a fault to fix (the window's fault, a red "!"
 * and its words), or a choice to make (a plain dash: nothing is wrong). None
 * is a filled disc: that is a proof. `what` is what it means, `actions` what to
 * do about it: for a fault the one primary button, then any other.
 */
function Step(props: { kind: "ok" | "fault" | "choice"; title: string; detail: string; what?: ReactNode; actions?: ReactNode }) {
  if (props.kind === "fault") {
    return (
      <div className="step">
        <Fault framed={false} title={props.title} detail={props.detail} action={props.actions}>
          {props.what}
        </Fault>
      </div>
    );
  }
  return (
    <div className={props.kind === "ok" ? "step ok" : "step choice"}>
      <span className="step-mark" aria-hidden="true">
        <Icon name={props.kind === "ok" ? "check" : "dash"} size={16} />
      </span>
      <div className="step-body">
        <div className="step-head">
          <b className="step-name">{props.title}</b>
          <span className="step-detail selectable">{props.detail}</span>
        </div>
        {props.what ? <p className="selectable">{props.what}</p> : null}
        {props.actions ? <div className="row">{props.actions}</div> : null}
      </div>
    </div>
  );
}

/** Whether the program is what stops a lesson, so that its step holds the screen's one green button. */
function runtimeBlocks(runtime: RuntimeSetup): boolean {
  return runtime.state !== "ready" && runtime.state !== "kept";
}

/**
 * The program's step. `lead` is true when this step holds up the lesson and is
 * the first that does: its action is the screen's one primary button.
 */
function RuntimeStep(props: { runtime: RuntimeSetup; lead: boolean; onReplace: () => void; onKeep: () => void; onCheck: () => void }) {
  const { runtime } = props;
  const installed = runtime.installed;
  const bundled = runtime.bundled;
  const where = installed ? installed.shown_path : "";
  if (runtime.state === "ready" && installed) {
    return <Step kind="ok" title="Magnesium" detail={`${versionNumber(installed.version)} at ${where}${runtime.installed_now ? ", installed just now" : ""}`} />;
  }
  if (runtime.state === "different" && installed && bundled) {
    const theirs = installed.version ? `version ${versionNumber(installed.version)}` : "a copy that does not run";
    return (
      <Step
        kind="choice"
        title="Magnesium"
        detail={`another copy at ${where}`}
        what={`This Mac has Magnesium ${theirs}. This app came with ${versionNumber(bundled.version)}. Replace it? The window, Claude Code and a terminal all use the one copy. The newer one upgrades a course's record when it opens the course, after asking you, and keeps a copy of it first.`}
        actions={
          <>
            <button className="btn primary" title="Replace (Return)" onClick={props.onReplace}>
              Replace
            </button>
            <button className="btn" onClick={props.onKeep}>
              Keep {installed.version ? versionNumber(installed.version) : "it"}
            </button>
          </>
        }
      />
    );
  }
  if (runtime.state === "too_old" && installed) {
    // Older than the window needs: it cannot be kept, so the only way on is a
    // newer copy, the app's own when it carries one.
    return (
      <Step
        kind="fault"
        title="Magnesium"
        detail={`${versionNumber(installed.version)}, too old, at ${where}`}
        what={runtime.message}
        actions={
          <>
            {bundled && (
              <button className="btn primary" title="Replace (Return)" onClick={props.onReplace}>
                Replace with {versionNumber(bundled.version)}
              </button>
            )}
            <button className={bundled ? "btn" : !props.lead ? "btn small" : "btn primary"} onClick={props.onCheck}>
              Check again
            </button>
          </>
        }
      />
    );
  }
  if (runtime.state === "kept" && installed && bundled) {
    return (
      <Step
        kind="ok"
        title="Magnesium"
        detail={`${versionNumber(installed.version)}, kept, at ${where}`}
        what={runtime.message}
        actions={
          <button className="btn small" onClick={props.onReplace}>
            Replace with {versionNumber(bundled.version)}
          </button>
        }
      />
    );
  }
  return (
    <Step
      kind="fault"
      title="Magnesium"
      detail={runtime.state === "missing" ? "not installed" : installed ? `${versionNumber(installed.version) || "does not run"} at ${where}` : "not installed yet"}
      what={runtime.message}
      actions={
        <button className={props.lead ? "btn primary" : "btn small"} onClick={props.onCheck}>
          Try again
        </button>
      }
    />
  );
}

/** `lead` is true when nothing before this step holds up the lesson: its action is the screen's one primary button. */
function AccountStep(props: { account: Account; lead: boolean; signingIn: boolean; onSignIn: () => void; onCheck: () => void; onLink: (url: string) => void }) {
  const { account } = props;
  const name = account.name || "Claude Code";
  if (!account.installed) {
    return (
      <Step
        kind="fault"
        title={name}
        detail="not found"
        what={`${account.problem ?? ""} Magnesium teaches through ${name}, which needs a paid Claude plan or an Anthropic Console account.`.trim()}
        actions={
          <>
            {account.install_page && (
              <button className={props.lead ? "btn primary" : "btn small"} onClick={() => props.onLink(account.install_page!)}>
                Anthropic&apos;s install page
                {!props.lead && <Icon name="external" size={14} />}
              </button>
            )}
            <button className={props.lead && !account.install_page ? "btn primary" : props.lead ? "btn" : "btn small"} onClick={props.onCheck}>
              Check again
            </button>
          </>
        }
      />
    );
  }
  const signIn = (
    <>
      <button className={props.lead ? "btn primary" : "btn small"} disabled={props.signingIn} onClick={props.onSignIn}>
        {props.signingIn ? "Waiting for the browser…" : "Sign in"}
      </button>
      <button className={props.lead ? "btn" : "btn small"} onClick={props.onCheck}>
        Check again
      </button>
    </>
  );
  const waiting = props.signingIn ? " Finish signing in on Anthropic's page in your browser. This screen moves on by itself." : "";
  if (!account.signed_in) {
    return <Step kind="fault" title={name} detail="signed out" what={`${account.problem ?? ""}${waiting}`.trim()} actions={signIn} />;
  }
  const detail = `signed in, pays with ${account.detail ?? "your account"}`;
  if (account.problem) {
    return <Step kind="fault" title={name} detail={detail} what={`${account.problem ?? ""}${waiting}`.trim()} actions={signIn} />;
  }
  return <Step kind="ok" title={name} detail={detail} />;
}

export function Setup(props: {
  setup: SetupData | null;
  about: About | null;
  error: string | null;
  signingIn: boolean;
  /** The library is behind this screen, so there is somewhere to go back to. */
  canGoBack: boolean;
  onBack: () => void;
  onPalette: () => void;
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
  useReturnForPrimary(setup !== null);
  const ready = setupReady(setup);
  const version = aboutText(props.about);
  // The one green button is the action that unblocks setup: the program's if that is what is wrong, else Claude Code's. Once setup is ready it is Done.
  const programFirst = setup ? runtimeBlocks(setup.runtime) : false;

  let title = "Before your first lesson";
  let sub = "Magnesium runs on this Mac and teaches through Claude Code.";
  if (!setup) {
    title = "One moment";
    sub = "Looking for Magnesium and Claude Code on this Mac.";
  } else if (setup.runtime.state === "too_old") {
    title = "Magnesium needs updating";
    sub = "The program on this Mac is older than this window needs.";
  } else if (ready) {
    title = "Ready";
    sub = setup.payment === "api_key" ? "Lessons are paid for with your API key, per use." : "Lessons use your Claude plan.";
  }

  return (
    <section className="screen setup" data-screen="setup">
      <TopBar
        left={
          props.canGoBack ? (
            <button className="crumb" onClick={props.onBack} title="Library (Esc)">
              <Icon name="back" size={16} />
              Library
            </button>
          ) : undefined
        }
        right={ready ? <PaletteKey onClick={props.onPalette} /> : undefined}
      />
      <div className="center top">
        <div className="setup-col">
          <h1 className="h-title">{title}</h1>
          <p className="setup-sub">{sub}</p>
          {setup && !ready && (
            <div className="steps">
              <RuntimeStep runtime={setup.runtime} lead={programFirst} onReplace={props.onReplace} onKeep={props.onKeep} onCheck={props.onCheck} />
              <AccountStep
                account={setup.account}
                lead={!programFirst}
                signingIn={props.signingIn}
                onSignIn={props.onSignIn}
                onCheck={props.onCheck}
                onLink={props.onLink}
              />
            </div>
          )}
          {setup && (
            <details className="advanced">
              <summary>
                <Icon name="right" size={14} />
                Advanced
              </summary>
              <div className="steps">
                {ready && (
                  <>
                    <RuntimeStep runtime={setup.runtime} lead={false} onReplace={props.onReplace} onKeep={props.onKeep} onCheck={props.onCheck} />
                    <AccountStep
                      account={setup.account}
                      lead={false}
                      signingIn={props.signingIn}
                      onSignIn={props.onSignIn}
                      onCheck={props.onCheck}
                      onLink={props.onLink}
                    />
                  </>
                )}
                <div className="step plain">
                  <span className="step-mark" aria-hidden="true" />
                  <div className="step-body">
                    <div className="step-head">
                      <span className="h-label">Pay for lessons with</span>
                    </div>
                    <Choice
                      label="Pay for lessons with"
                      value={setup.payment}
                      options={[
                        { value: "plan", label: "Your Claude plan" },
                        { value: "api_key", label: "An API key, per use" },
                      ]}
                      onChange={props.onPayment}
                    />
                  </div>
                </div>
                {props.engine && (
                  <div className="step plain">
                    <span className="step-mark" aria-hidden="true" />
                    <div className="step-body">
                      <div className="step-head">
                        <span className="h-label">Lessons run on</span>
                      </div>
                      {props.engine}
                    </div>
                  </div>
                )}
              </div>
              {version && <p className="aside version selectable">{version}</p>}
              <p className="aside selectable">
                Setting a course up also writes two things outside it: Magnesium&apos;s plugin for Claude Code, in ~/Library/Application
                Support/magnesium, and one entry in Claude Code&apos;s settings that names it.
              </p>
            </details>
          )}
          {props.error && <Fault>{props.error}</Fault>}
          {/* While a step above is open, its own action is the one green button; the way out is said in words, never as a button that cannot be pressed. */}
          {ready ? (
            <div className="row center">
              <button className="btn primary" title="Done (Return)" onClick={props.onBack} data-role="setup-done">
                {props.canGoBack ? "Done" : "Go to your library"}
              </button>
            </div>
          ) : (
            setup && <p className="aside">{props.canGoBack ? "Once these steps are done, Done takes you back to the library." : "Once these steps are done, you can go to your library."}</p>
          )}
        </div>
      </div>
    </section>
  );
}
