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
    pickNewFolder: async () => (await dialog.save({ title: "Make a new study folder", defaultPath: "My course" })) ?? null,
    openFolder: (path) => invoke("open_folder", { path }),
    newFolder: (path) => invoke("new_folder", { path }),
    home: () => invoke("home"),
    setUp: () => invoke("set_up"),
    upgradeFolder: () => invoke("upgrade_folder"),
    setup: () => invoke("setup"),
    replaceRuntime: () => invoke("replace_runtime"),
    keepRuntime: () => invoke("keep_runtime"),
    account: () => invoke("account"),
    setPayment: (payment) => invoke("set_payment", { payment }),
    signIn: () => invoke("sign_in"),
    about: () => invoke("about"),
    openSession: (resume) => invoke("open_session", { resume }),
    send: (text) => invoke("send", { text }),
    sendNewLesson: (goal) => invoke("send_new_lesson", { goal }),
    sendShown: (shown, sent) => invoke("send_shown", { shown, sent }),
    interrupt: () => invoke("interrupt"),
    decide: (id, decision) => invoke("decide", { id, decision }),
    answer: (id, answers) => invoke("answer", { id, answers }),
    stop: (stoppedAt) => invoke("stop", { stoppedAt }),
    rail: () => invoke("rail"),
    progress: (target) => invoke("progress", { target }),
    setProgressView: (view) => invoke("set_progress_view", { view }),
    setBlockMinutes: (minutes) => invoke("set_block_minutes", { minutes }),
    attach: (path, group) => invoke("attach", { path, group }),
    openFile: (path) => invoke("open_file", { path }),
    openUrl: (url) => invoke("open_url", { url }),
    image: (path) => invoke("image", { path }),
    paperStart: (paper, minutes) => invoke("paper_start", { paper, minutes }),
    paperEnd: (files) => invoke("paper_end", { files }),
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
