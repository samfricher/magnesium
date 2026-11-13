// The tab bar, from the workspace editor concept (#68): one quiet row at the
// top, beside the traffic lights, that the window is dragged by. Home is
// always first; then the open sessions; then Settings when it is open. Its
// right end holds the plan's switch (in a session) and Settings.
//
// It is a tablist that holds tabs only, one Tab stop: ← → move along it,
// Home and End go to its ends, Return or Space shows the tab, Delete closes
// it. Showing a tab is by activation, not by focus, because showing one
// notes when its session was opened and keeps how the window was left. The
// close marks are for the pointer; the keyboard closes with Delete, ⌘W or
// the menu. A tab too far along to fit is brought into view when it is
// shown or focused, and the Window menu lists every tab.

import { memo, useEffect, useRef } from "react";

import { Line } from "../surface/Rich";

export type TabView = { key: string; kind: "home" | "session" | "settings"; title: string; busy: boolean; folder: string | null };

/** The id of a tab's button, which its panel is labelled by. */
export const tabId = (key: string) => `tab-${key}`;

type Props = {
  tabs: TabView[];
  active: string;
  /** The plan is shown beside the lesson; null when the tab in front is not a session. */
  plan: boolean | null;
  planKey: string;
  settingsKey: string;
  newKey: string;
  onPick: (key: string) => void;
  onClose: (key: string) => void;
  onNew: () => void;
  onPlan: () => void;
  onSettings: () => void;
};

export const Tabs = memo(function Tabs(props: Props) {
  const list = useRef<HTMLDivElement>(null);
  const buttons = () => Array.from(list.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? []);

  // The tab shown is always in view, however many are open.
  useEffect(() => {
    list.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [props.active, props.tabs.length]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>, at: number) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const all = buttons();
    const go = (to: number) => {
      event.preventDefault();
      all[(to + all.length) % all.length]?.focus();
    };
    if (event.key === "ArrowRight") go(at + 1);
    else if (event.key === "ArrowLeft") go(at - 1);
    else if (event.key === "Home") go(0);
    else if (event.key === "End") go(all.length - 1);
    else if ((event.key === "Delete" || event.key === "Backspace") && props.tabs[at]?.kind !== "home") {
      event.preventDefault();
      // Focus stays in the bar: on the tab before the one closed.
      all[Math.max(0, at - 1)]?.focus();
      props.onClose(props.tabs[at].key);
    }
  };

  return (
    <header className="tabbar" data-tauri-drag-region>
      <div className="tabs" role="tablist" aria-label="Open tabs" ref={list} data-tauri-drag-region>
        {props.tabs.map((tab, at) => {
          const here = tab.key === props.active;
          return (
            <span key={tab.key} className={`tab${here ? " here" : ""}${tab.kind === "home" ? " home" : ""}`} data-kind={tab.kind}>
              <button
                type="button"
                role="tab"
                id={tabId(tab.key)}
                aria-selected={here}
                tabIndex={here ? 0 : -1}
                className="tab-button"
                data-drive={`tab-${at + 1}`}
                title={`${tab.title}${at < 9 ? `   ⌘${at + 1}` : ""}`}
                onClick={() => props.onPick(tab.key)}
                onKeyDown={(event) => onKeyDown(event, at)}
                onAuxClick={(event) => event.button === 1 && tab.kind !== "home" && props.onClose(tab.key)}
              >
                {tab.busy && <span className="live-dot" aria-hidden="true" />}
                {tab.kind === "home" ? (
                  <span className="tab-name" data-drive="home">
                    Home
                  </span>
                ) : (
                  <span className="tab-name">
                    <Line text={tab.title} />
                  </span>
                )}
                {tab.busy && <span className="visually-hidden">, the tutor is writing</span>}
              </button>
              {tab.kind !== "home" && (
                // For the pointer only: a tablist holds tabs, so the keyboard closes a tab with Delete or ⌘W.
                <button
                  type="button"
                  className="tab-close"
                  data-drive={`close-tab-${at + 1}`}
                  aria-hidden="true"
                  tabIndex={-1}
                  title="Close   ⌘W"
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => props.onClose(tab.key)}
                >
                  ×
                </button>
              )}
            </span>
          );
        })}
      </div>
      <button type="button" className="tab-new" data-drive="new-session" aria-label="New session" title={`New session   ${props.newKey}`} onClick={props.onNew}>
        +
      </button>
      <div className="tabbar-end">
        {props.plan !== null && (
          <button
            type="button"
            className={props.plan ? "bar-act on" : "bar-act"}
            data-drive="plan-toggle"
            aria-expanded={props.plan}
            aria-controls={props.plan ? "plan-panel" : undefined}
            title={`${props.plan ? "Hide" : "Show"} the plan   ${props.planKey}`}
            onClick={props.onPlan}
          >
            Plan
          </button>
        )}
        <button type="button" className={props.active === "settings" ? "bar-act current" : "bar-act"} data-drive="settings" title={`Settings   ${props.settingsKey}`} onClick={props.onSettings}>
          Settings
        </button>
      </div>
    </header>
  );
});
