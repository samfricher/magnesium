import { memo, useEffect, useRef } from "react";

import { Line } from "../surface/Rich";

export type TabView = {
  key: string;
  kind: "home" | "session" | "settings";
  title: string;
  keyName: string;
  busy: boolean;
  asking: boolean;
  folder: string | null;
};

export const tabId = (key: string) => `tab-${key}`;

type Props = {
  tabs: TabView[];
  active: string;
  plan: boolean | null;
  planKey: string;
  flip: string | null;
  flipKey: string;
  onFlip: () => void;
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

  useEffect(() => {
    list.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [props.active, props.tabs.length]);

  const settingsOpen = props.tabs.some((tab) => tab.kind === "settings");
  const keysOnSettings = useRef(false);
  useEffect(() => {
    if (props.active !== "settings" || !keysOnSettings.current) return;
    keysOnSettings.current = false;
    document.getElementById(tabId("settings"))?.focus();
  }, [props.active]);

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
    else if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      all[at > 0 ? at - 1 : 1]?.focus();
      props.onClose(props.tabs[at].key);
    }
  };

  return (
    <header className="tabbar" data-tauri-drag-region onMouseDown={(event) => event.preventDefault()}>
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
                title={`${tab.title}${tab.keyName ? `   ${tab.keyName}` : ""}`}
                onClick={() => props.onPick(tab.key)}
                onKeyDown={(event) => onKeyDown(event, at)}
                onAuxClick={(event) => event.button === 1 && props.onClose(tab.key)}
              >
                {tab.asking ? <span className="ask-dot" aria-hidden="true" /> : tab.busy && <span className="live-dot" aria-hidden="true" />}
                {tab.kind === "home" ? (
                  <span className="tab-name" data-drive="home">
                    <Line text={tab.title} />
                  </span>
                ) : (
                  <span className="tab-name">
                    <Line text={tab.title} />
                  </span>
                )}
                {tab.asking ? <span className="visually-hidden">, the tutor waits for you</span> : tab.busy && <span className="visually-hidden">, the tutor is writing</span>}
              </button>
              <button type="button" className="tab-close" data-drive={`close-tab-${at + 1}`} aria-hidden="true" tabIndex={-1} title="Close   ⌘W" onClick={() => props.onClose(tab.key)}>
                ×
              </button>
            </span>
          );
        })}
      </div>
      <button type="button" className="tab-new" data-drive="new-tab" aria-label="New tab" title={`New tab   ${props.newKey}`} onClick={props.onNew}>
        +
      </button>
      <div className="tabbar-end">
        {props.flip !== null && (
          <button
            type="button"
            className="bar-act"
            data-drive="flip"
            title={`Switch to the ${props.flip.toLowerCase()}   ${props.flipKey}`}
            onClick={props.onFlip}
          >
            {props.flip}
          </button>
        )}
        {props.plan !== null && (
          <button
            type="button"
            className={props.plan ? "bar-act on" : "bar-act"}
            data-drive="plan-toggle"
            aria-expanded={props.plan}
            aria-controls={props.plan ? "plan-panel" : undefined}
            title={`${props.plan ? "Hide" : "Show"} the plan and the documents open in this session   ${props.planKey}`}
            onClick={props.onPlan}
          >
            Plan
          </button>
        )}
        {!settingsOpen && (
          <button
            type="button"
            className="bar-act"
            data-drive="settings"
            title={`Settings   ${props.settingsKey}`}
            onClick={(event) => {
              keysOnSettings.current = document.activeElement === event.currentTarget;
              props.onSettings();
            }}
          >
            Settings
          </button>
        )}
      </div>
    </header>
  );
});
