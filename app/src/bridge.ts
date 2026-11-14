// The page's one way to the core. Inside the window it calls the Rust shell.
// In a plain browser it uses the demo, which holds made-up data, so the
// screens can be worked on and photographed without starting an agent.

import type { Core } from "./types";

const inWindow = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriCore(): Promise<Core> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const dialog = await import("@tauri-apps/plugin-dialog");
  const list = (picked: string | string[] | null): string[] =>
    picked === null ? [] : Array.isArray(picked) ? picked : [picked];
  return {
    initialFolder: () => invoke("initial_folder"),
    recent: () => invoke("recent"),
    library: () => invoke("library"),
    workspaces: () => invoke("workspaces"),
    forgetFolder: (path) => invoke("forget_folder", { path }),
    findFolders: () => invoke("find_folders"),
    activity: () => invoke("activity"),
    pickFolder: async () => list(await dialog.open({ directory: true, title: "Open a study folder" }))[0] ?? null,
    pickFiles: async () =>
      list(
        await dialog.open({
          multiple: true,
          title: "Photos or a PDF of your work",
          filters: [
            { name: "Photos and PDFs", extensions: ["jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp", "bmp", "pdf"] },
          ],
        }),
      ),
    pickMaterial: async () => list(await dialog.open({ multiple: true, title: "Add course material" })),
    pickNewFolder: async () => (await dialog.save({ title: "Make a new study folder", defaultPath: "My course" })) ?? null,
    openFolder: (path) => invoke("open_folder", { path }),
    visitFolder: (path) => invoke("visit_folder", { path }),
    newFolder: (path) => invoke("new_folder", { path }),
    home: (folder) => invoke("home", { folder: folder ?? null }),
    setUp: (folder) => invoke("set_up", { folder: folder ?? null }),
    upgradeFolder: (folder) => invoke("upgrade_folder", { folder: folder ?? null }),
    setup: () => invoke("setup"),
    replaceRuntime: () => invoke("replace_runtime"),
    keepRuntime: () => invoke("keep_runtime"),
    account: () => invoke("account"),
    setPayment: (payment) => invoke("set_payment", { payment }),
    engine: () => invoke("engine"),
    setEngine: (engine) => invoke("set_engine", { engine }),
    signIn: () => invoke("sign_in"),
    about: () => invoke("about"),
    turns: () => invoke("turns"),
    lessons: (folder) => invoke("lessons", { folder }),
    lessonTurns: (folder, session) => invoke("lesson_turns", { folder, session }),
    touchSession: (folder, session) => invoke("touch_session", { folder, session }),
    renameSession: (folder, session, title) => invoke("rename_session", { folder, session, title }),
    shareFile: (path, folder, at) => invoke("share_file", { folder, path, ...at }),
    openSession: (resume, where) =>
      invoke("open_session", { resume, folder: where?.folder ?? null, session: where?.session ?? null, title: where?.title ?? null }),
    send: (text, intent) => invoke("send", { text, intent: intent ?? null }),
    sendNewLesson: (goal) => invoke("send_new_lesson", { goal }),
    sendShown: (shown, sent) => invoke("send_shown", { shown, sent }),
    interrupt: () => invoke("interrupt"),
    decide: (id, decision) => invoke("decide", { id, decision }),
    answer: (id, answers) => invoke("answer", { id, answers }),
    stop: (stoppedAt) => invoke("stop", { stoppedAt }),
    rail: () => invoke("rail"),
    plan: (folder) => invoke("plan", { folder: folder ?? null }),
    progress: (target, folder) => invoke("progress", { target, folder: folder ?? null }),
    setBlockMinutes: (minutes) => invoke("set_block_minutes", { minutes }),
    attach: (path, group, folder) => invoke("attach", { path, group, folder: folder ?? null }),
    attachBytes: (name, bytes, group, folder) => invoke("attach_bytes", { name, bytes: Array.from(bytes), group, folder: folder ?? null }),
    addMaterial: (paths, folder) => invoke("add_material", { paths, folder: folder ?? null }),
    addPastedText: (text, folder) => invoke("add_pasted_text", { text, folder: folder ?? null }),
    materialList: (folder) => invoke("material_list", { folder: folder ?? null }),
    marks: (folder) => invoke("marks", { folder: folder ?? null }),
    onDrop: async (handler, hover) => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      return getCurrentWebview().onDragDropEvent((event) => {
        const drag = event.payload;
        if (drag.type === "enter" || drag.type === "over") hover(true);
        else if (drag.type === "leave") hover(false);
        else if (drag.type === "drop") {
          hover(false);
          handler(drag.paths);
        }
      });
    },
    source: (path, folder) => invoke("source", { path, folder: folder ?? null }),
    sourcePdf: (path, folder) => invoke("source_pdf", { path, folder: folder ?? null }),
    openFile: (path, folder) => invoke("open_file", { path, folder: folder ?? null }),
    openUrl: (url) => invoke("open_url", { url }),
    image: (path, folder) => invoke("image", { path, folder: folder ?? null }),
    paperStart: (paper, minutes, folder) => invoke("paper_start", { paper, minutes, folder: folder ?? null }),
    paperEnd: (files, folder) => invoke("paper_end", { files, folder: folder ?? null }),
    windowState: () => invoke("window_state"),
    setWindowState: (value) => invoke("set_window_state", { value }),
    setMenu: (spec) => invoke("set_menu", { spec }),
    onMenu: (handler) => listen("menu", (event) => handler(event.payload as never)),
    onEvent: (handler) => listen("agent", (event) => handler(event.payload as never)),
    selftest: () => invoke("selftest"),
    selftestReport: (report) => invoke("selftest_report", { report }),
  };
}

let ready: Promise<Core> | null = null;

export function core(): Promise<Core> {
  ready ??= inWindow ? tauriCore() : import("./demo").then((demo) => demo.demoCore());
  return ready;
}

export const isDemo = !inWindow;
