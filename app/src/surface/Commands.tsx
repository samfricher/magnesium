// ⌘K: the few commands of the window itself, and nothing else. Home, another
// workspace, a new session, closing this one, Settings, and the plan beside
// the lesson. Settings' values, papers, material and the folder's own
// commands live on the Settings and workspace pages. Every row here is also
// a visible control and an item in the menu bar.

import { useState } from "react";

import type { Workspace } from "../types";
import { displayName } from "../words";
import { Layer, type Row } from "./Layer";

type Props = {
  workspaces: Workspace[];
  keys: { home: string; newSession: string; closeSession: string; settings: string; plan: string; openWorkspace: string };
  /** A session is in front, so it can be closed and its plan shown. */
  inSession: boolean;
  planShown: boolean;
  onClose: () => void;
  run: (command: string, arg?: string) => void;
};

export function Commands(props: Props) {
  const [switching, setSwitching] = useState(false);
  const act = (command: string, arg?: string) => () => {
    props.onClose();
    props.run(command, arg);
  };
  if (switching) {
    const rows: Row[] = props.workspaces.map((workspace) => ({
      id: workspace.folder,
      kind: "thing",
      label: displayName(workspace.name),
      text: `${workspace.name} ${displayName(workspace.name)}`,
      drive: `switch-${displayName(workspace.name)}`,
      run: act("go", workspace.folder),
    }));
    rows.push({ id: "open", label: "Open a folder…", text: "open folder choose another", hint: props.keys.openWorkspace, run: act("open-workspace") });
    return <Layer key="switch" name="Switch workspace" mode="filter" placeholder="A workspace" rows={rows} keys="↩ open its page   esc close" onClose={props.onClose} />;
  }
  const rows: Row[] = [
    { id: "home", label: "Home", text: "home start workspaces library", hint: props.keys.home, drive: "command-home", run: act("home") },
    { id: "switch", label: "Switch workspace…", text: "switch workspace course folder open another go", drive: "command-switch", run: () => setSwitching(true) },
    { id: "new", label: "New session", text: "new session lesson start", hint: props.keys.newSession, drive: "command-new-session", run: act("new-session") },
    ...(props.inSession ? [{ id: "close", label: "Close session", text: "close session stop end tab", hint: props.keys.closeSession, drive: "command-close-session", run: act("close-session") }] : []),
    { id: "settings", label: "Settings", text: "settings preferences font size colour keys engine", hint: props.keys.settings, drive: "command-settings", run: act("settings") },
    ...(props.inSession
      ? [{ id: "plan", label: props.planShown ? "Hide the plan" : "Show the plan", text: "plan show hide panel tasks", hint: props.keys.plan, drive: "command-plan", run: act("plan") }]
      : []),
  ];
  return <Layer key="commands" name="Commands" mode="filter" placeholder="A command" rows={rows} keys="↩ do it   esc close" onClose={props.onClose} />;
}
