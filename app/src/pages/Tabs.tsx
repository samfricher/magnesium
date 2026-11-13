// The tab bar, from the workspace editor concept (#68): one quiet row at the
// top, beside the traffic lights, that the window is dragged by. Home is
// always first; then the open sessions; then Settings when it is open. Its
// right end holds the plan's switch (in a session) and Settings.

import { memo } from "react";

import { Line } from "../surface/Rich";

export type TabView = { key: string; kind: "home" | "session" | "settings"; title: string; busy: boolean; folder: string | null };

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
  return (
    <header className="tabbar" data-tauri-drag-region>
      <div className="tabs" role="tablist" aria-label="Tabs" data-tauri-drag-region>
        {props.tabs.map((tab, at) => {
          const here = tab.key === props.active;
          return (
            <span key={tab.key} className={`tab${here ? " here" : ""}${tab.kind === "home" ? " home" : ""}`} data-kind={tab.kind}>
              <button
                role="tab"
                aria-selected={here}
                className="tab-button"
                data-drive={`tab-${at + 1}`}
                title={`${tab.title}${at < 9 ? `   ⌘${at + 1}` : ""}`}
                onClick={() => props.onPick(tab.key)}
                onAuxClick={(event) => event.button === 1 && tab.kind !== "home" && props.onClose(tab.key)}
              >
                {tab.busy && <span className="live-dot" aria-label="The tutor is writing" />}
                {tab.kind === "home" ? (
                  <span className="tab-name" data-drive="home">
                    Home
                  </span>
                ) : (
                  <span className="tab-name">
                    <Line text={tab.title} />
                  </span>
                )}
              </button>
              {tab.kind !== "home" && (
                <button className="tab-close" data-drive={`close-tab-${at + 1}`} aria-label={`Close ${tab.title}`} title="Close   ⌘W" onClick={() => props.onClose(tab.key)}>
                  ×
                </button>
              )}
            </span>
          );
        })}
        <button className="tab-new" data-drive="new-session" aria-label="New session" title={`New session   ${props.newKey}`} onClick={props.onNew}>
          +
        </button>
      </div>
      <div className="tabbar-end">
        {props.plan !== null && (
          <button className={props.plan ? "bar-act on" : "bar-act"} data-drive="plan-toggle" aria-pressed={props.plan} title={`${props.plan ? "Hide" : "Show"} the plan   ${props.planKey}`} onClick={props.onPlan}>
            Plan
          </button>
        )}
        <button className="bar-act" data-drive="settings" title={`Settings   ${props.settingsKey}`} onClick={props.onSettings}>
          Settings
        </button>
      </div>
    </header>
  );
});
