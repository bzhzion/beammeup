import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

interface SessionInfo {
  id: string;
  kind: string;
  title: string;
  label: string | null;
  alive: boolean;
}

interface ShellDescriptor {
  id: string;
  label: string;
  program: string;
  args: string[];
}

interface DataEvent {
  id: string;
  chunk: string;
}

interface Tab {
  info: SessionInfo;
  term: Terminal;
  fit: FitAddon;
  container: HTMLDivElement;
  row: HTMLButtonElement;
  colorIndex: number;
}

const tabs = new Map<string, Tab>();
let activeId: string | null = null;
let fullscreen = false;
let nextTabColor = 0;
const TAB_COLOR_COUNT = 5;
// Same rotation as the `.tab-color-N` CSS classes: repeated here to color the active session's
// frame (`--active-color`) in JS, where applying a CSS class alone would not be enough since the
// color needs to be read by several elements (banner + border) via a shared variable.
const TAB_COLOR_VARS = [
  "var(--orange)",
  "var(--periwinkle)",
  "var(--violet)",
  "var(--blue)",
  "var(--red-alert)",
];

// A single resize listener for the whole application, instead of one per tab: the previous
// version added one on every session creation and never removed it on close, so they kept piling
// up for as long as the window stayed open.
window.addEventListener("resize", () => {
  if (!activeId) return;
  tabs.get(activeId)?.fit.fit();
});

const tabsEl = document.getElementById("tabs") as HTMLDivElement;
const panesEl = document.getElementById("panes") as HTMLDivElement;
const shellPicker = document.getElementById("shell-picker") as HTMLSelectElement;
const openShellBtn = document.getElementById("open-shell") as HTMLButtonElement;
const sshInput = document.getElementById("ssh-input") as HTMLInputElement;
const openSshBtn = document.getElementById("open-ssh") as HTMLButtonElement;
const fullscreenBtn = document.getElementById("fullscreen-toggle") as HTMLButtonElement;
const brandVersionEl = document.getElementById("brand-version") as HTMLSpanElement;
const sessionFrame = document.querySelector(".session-frame") as HTMLElement;
const sessionFrameTitle = document.getElementById("session-frame-title") as HTMLSpanElement;

function activate(id: string) {
  for (const tab of tabs.values()) {
    const isActive = tab.info.id === id;
    tab.container.classList.toggle("active", isActive);
    tab.row.classList.toggle("active", isActive);
    tab.row.setAttribute("aria-current", isActive ? "true" : "false");
  }
  activeId = id;
  const tab = tabs.get(id);
  if (tab) {
    tab.fit.fit();
    tab.term.focus();
    void invoke("ui_resize", { id, cols: tab.term.cols, rows: tab.term.rows }).catch(() => {});
    sessionFrame.style.setProperty("--active-color", TAB_COLOR_VARS[tab.colorIndex]);
    sessionFrameTitle.textContent = tab.info.label
      ? `${tab.info.label} · ${tab.info.title}`
      : tab.info.title;
  }
}

/// Creates the tab for a session if it does not exist yet. Called both for a session opened from
/// the UI and for a session opened externally by the agent via the CLI: both go through the same
/// `session-opened` event, so both appear the same way, with no duplicated code and no risk of
/// the agent's session being missed.
function ensureTab(info: SessionInfo): Tab {
  const existing = tabs.get(info.id);
  if (existing) return existing;

  const container = document.createElement("div");
  container.className = "pane";
  panesEl.appendChild(container);

  const term = new Terminal({
    convertEol: true,
    fontFamily: "JetBrainsMono, Cascadia Code, Consolas, monospace",
    fontSize: 14,
    cursorBlink: true,
    theme: {
      background: "#050506",
      foreground: "#f4f1e8",
      cursor: "#ff9f1c",
      selectionBackground: "#3f3560",
    },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(container);
  fit.fit();

  term.onData((data) => {
    void invoke("ui_send", { id: info.id, text: data }).catch(() => {});
  });
  term.onResize(({ cols, rows }) => {
    void invoke("ui_resize", { id: info.id, cols, rows }).catch(() => {});
  });

  const row = document.createElement("button");
  row.setAttribute("role", "listitem");
  row.setAttribute("aria-current", "false");
  // Flat LCARS color assigned on creation, in a simple rotation: never a rainbow gradient on the
  // sessions themselves (that is reserved for the logo), just a solid color swatch per session so
  // they can be told apart at a glance in the vertical list.
  const colorIndex = nextTabColor % TAB_COLOR_COUNT;
  nextTabColor += 1;
  row.className = "tab-row";
  row.title = info.title;

  const swatch = document.createElement("span");
  swatch.className = `tab-swatch tab-color-${colorIndex}`;
  swatch.setAttribute("aria-hidden", "true");
  row.appendChild(swatch);

  const labelSpan = document.createElement("span");
  labelSpan.className = "tab-label";
  labelSpan.textContent = info.label ? `${info.label} · ${info.title}` : info.title;
  row.appendChild(labelSpan);

  const closeSpan = document.createElement("span");
  closeSpan.className = "tab-close";
  closeSpan.textContent = "×";
  closeSpan.title = "Close session";
  closeSpan.addEventListener("click", (e) => {
    e.stopPropagation();
    void invoke("ui_close", { id: info.id }).catch(() => {});
  });
  row.appendChild(closeSpan);

  row.addEventListener("click", () => activate(info.id));
  tabsEl.appendChild(row);

  const tab: Tab = { info, term, fit, container, row, colorIndex };
  tabs.set(info.id, tab);

  // Replays the existing scrollback (useful if the session had already produced output before
  // this tab was created, e.g. a `beammeup send` right after a `beammeup open`).
  void invoke<[string, number, boolean]>("ui_read", { id: info.id, since: 0 }).then(([data]) => {
    if (data) term.write(data);
  });

  return tab;
}

/// Removes a tab whose session has been closed (`beammeup close`, the × button, or the process
/// ending server-side): without this, the tab bar would just keep accumulating dead entries over
/// the course of a work session, making navigation unreadable as soon as many sessions get opened
/// and closed in a day.
function removeTab(id: string) {
  const tab = tabs.get(id);
  if (!tab) return;
  tab.row.remove();
  tab.container.remove();
  tab.term.dispose();
  tabs.delete(id);

  if (activeId === id) {
    const next = tabs.keys().next().value;
    if (next) {
      activate(next);
    } else {
      activeId = null;
      sessionFrame.style.setProperty("--active-color", "var(--orange)");
      sessionFrameTitle.textContent = "No session";
    }
  }
}

listen<SessionInfo>("session-opened", (event) => {
  ensureTab(event.payload);
  activate(event.payload.id);
});

listen<string>("session-closed", (event) => {
  removeTab(event.payload);
});

listen<string>("select-tab", (event) => {
  if (tabs.has(event.payload)) {
    activate(event.payload);
  }
});

listen<DataEvent>("session-data", (event) => {
  const tab = tabs.get(event.payload.id);
  if (tab) {
    tab.term.write(event.payload.chunk);
  }
});

async function loadShells() {
  const shells = await invoke<ShellDescriptor[]>("ui_shells");
  shellPicker.innerHTML = "";
  for (const s of shells) {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = s.label;
    shellPicker.appendChild(opt);
  }
  return shells;
}

openShellBtn.addEventListener("click", () => {
  const shell = shellPicker.value;
  if (!shell) return;
  void invoke("ui_open", { shell, ssh: null, label: null }).catch((e) => {
    console.error("failed to open tab", e);
  });
});

openSshBtn.addEventListener("click", () => {
  const args = sshInput.value.trim();
  if (!args) return;
  void invoke("ui_open", { shell: null, ssh: args, label: null })
    .then(() => {
      sshInput.value = "";
    })
    .catch((e) => {
      console.error("failed to connect via ssh", e);
    });
});
sshInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") openSshBtn.click();
});

fullscreenBtn.addEventListener("click", () => {
  fullscreen = !fullscreen;
  void invoke("ui_set_fullscreen", { enabled: fullscreen });
  fullscreenBtn.classList.toggle("active", fullscreen);
});

// Final title set here rather than on the Rust side: `window.set_title` would get overwritten
// there anyway by this same `<title>` tag once the page loads (WebView2 resyncs the native title
// bar to `document.title`, confirmed on 2026-08-25 on the real installer).
void (async () => {
  const version = await invoke<string>("ui_version");
  document.title = `BeamMeUp ${version}`;
  brandVersionEl.textContent = `v${version}`;
})();

// On the window's first launch, if no tab exists yet (no CLI already opened one during startup),
// open the first detected shell by default so the window is never empty.
void (async () => {
  const shells = await loadShells();
  const existing = await invoke<SessionInfo[]>("ui_list");
  if (existing.length === 0 && shells.length > 0) {
    await invoke("ui_open", { shell: shells[0].id, ssh: null, label: null });
  } else {
    existing.forEach((info) => ensureTab(info));
    if (existing.length > 0) activate(existing[0].id);
  }
})();

// -----------------------------------------------------------------------------
// Snippets: until now only manageable through the CLI (`beammeup snippet
// add/list/run/remove`), no interface for the human inside the window itself.
// A selection menu rather than a fully displayed list: with many snippets
// saved, a list would take up the sidebar's entire height.
// -----------------------------------------------------------------------------

const snippetPicker = document.getElementById("snippet-picker") as HTMLSelectElement;
const snippetRunBtn = document.getElementById("snippet-run") as HTMLButtonElement;
const snippetEditBtn = document.getElementById("snippet-edit-btn") as HTMLButtonElement;
const snippetAddBtn = document.getElementById("snippet-add-btn") as HTMLButtonElement;
const snippetDeleteBtn = document.getElementById("snippet-delete-btn") as HTMLButtonElement;
const snippetAddForm = document.getElementById("snippet-add-form") as HTMLDivElement;
const snippetNameInput = document.getElementById("snippet-name-input") as HTMLInputElement;
const snippetTextInput = document.getElementById("snippet-text-input") as HTMLInputElement;
const snippetAddConfirm = document.getElementById("snippet-add-confirm") as HTMLButtonElement;
const snippetAddCancel = document.getElementById("snippet-add-cancel") as HTMLButtonElement;

let snippetsCache = new Map<string, string>();
// The Delete button passes through a "Confirm?" state before actually acting (see
// `armDeleteConfirmation`): this timer cancels that state if the user does not click again within
// the following few seconds, so a "ready to delete" button is never left armed indefinitely.
let deleteConfirmTimer: number | undefined;

async function loadSnippets(selectName?: string) {
  const snippets = await invoke<[string, string][]>("ui_snippet_list");
  snippetsCache = new Map(snippets);
  snippetPicker.innerHTML = "";

  const hasSnippets = snippets.length > 0;
  snippetPicker.disabled = !hasSnippets;
  snippetRunBtn.disabled = !hasSnippets;
  snippetEditBtn.disabled = !hasSnippets;
  snippetDeleteBtn.disabled = !hasSnippets;
  resetDeleteConfirmation();

  if (!hasSnippets) {
    const opt = document.createElement("option");
    opt.textContent = "No snippet saved";
    snippetPicker.appendChild(opt);
    return;
  }
  for (const [name] of snippets) {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    snippetPicker.appendChild(opt);
  }
  if (selectName && snippetsCache.has(selectName)) {
    snippetPicker.value = selectName;
  }
}

function resetDeleteConfirmation() {
  window.clearTimeout(deleteConfirmTimer);
  snippetDeleteBtn.classList.remove("confirming");
  snippetDeleteBtn.textContent = "Delete";
}

function openSnippetForm(name = "", text = "") {
  snippetNameInput.value = name;
  snippetTextInput.value = text;
  snippetAddForm.classList.remove("hidden");
  snippetNameInput.focus();
}

function closeSnippetForm() {
  snippetAddForm.classList.add("hidden");
  snippetNameInput.value = "";
  snippetTextInput.value = "";
}

snippetRunBtn.addEventListener("click", () => {
  if (!activeId) return;
  const text = snippetsCache.get(snippetPicker.value);
  if (!text) return;
  // Enter by default, like `beammeup snippet run` without --no-enter.
  void invoke("ui_send", { id: activeId, text: `${text}\r` }).catch((err) => {
    console.error("failed to send snippet", err);
  });
});

snippetEditBtn.addEventListener("click", () => {
  const name = snippetPicker.value;
  const text = snippetsCache.get(name);
  if (text === undefined) return;
  openSnippetForm(name, text);
});

snippetAddBtn.addEventListener("click", () => openSnippetForm());
snippetAddCancel.addEventListener("click", () => closeSnippetForm());

// Double-click confirmation (see the "confirmation before destructive action" rule): the first
// click arms the button, the second one (within 4s) actually deletes. The button is disabled for
// the duration of the call itself so a double-tap on delete is never possible.
snippetDeleteBtn.addEventListener("click", async () => {
  if (!snippetDeleteBtn.classList.contains("confirming")) {
    snippetDeleteBtn.classList.add("confirming");
    snippetDeleteBtn.textContent = "Confirm?";
    deleteConfirmTimer = window.setTimeout(resetDeleteConfirmation, 4000);
    return;
  }

  window.clearTimeout(deleteConfirmTimer);
  const name = snippetPicker.value;
  snippetDeleteBtn.disabled = true;
  try {
    await invoke("ui_snippet_remove", { name });
  } catch (err) {
    console.error("failed to delete snippet", err);
  }
  await loadSnippets();
});

snippetAddConfirm.addEventListener("click", async () => {
  const name = snippetNameInput.value.trim();
  const text = snippetTextInput.value;
  if (!name || !text) return;
  await invoke("ui_snippet_add", { name, text }).catch((err) => {
    console.error("failed to save snippet", err);
  });
  closeSnippetForm();
  await loadSnippets(name);
});

void loadSnippets();
