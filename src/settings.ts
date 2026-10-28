import "./styles/tokens.css";
import "./styles/settings.css";

import { api, errorText } from "./lib/api";
import { escapeHtml, formatAgo, planLabel, shortcutKeys, shortenPath } from "./lib/format";
import { icons, logo } from "./lib/icons";
import { afterRender } from "./lib/frame";
import { recordKey } from "./lib/keys";
import { setTheme } from "./lib/theme";
import type { AppSnapshot, ProviderDetails, ProviderState, RefreshInterval, Settings, Theme } from "./lib/types";

const DEFAULT_SHORTCUT = "Ctrl+Shift+Space";
const app = document.getElementById("app")!;

let snapshot: AppSnapshot | null = null;
let details: ProviderDetails | null = null;
let recording = false;
let shortcutMessage: { text: string; error: boolean } | null = null;
const busy = new Set<string>();

const sw = (id: string, label: string) =>
  `<button class="switch" role="switch" id="${id}" aria-checked="false" aria-label="${label}"></button>`;
const item = (title: string, desc: string, control: string, extra = "") =>
  `<div class="item"${extra}><div class="item-text"><div class="item-title">${title}</div><div class="item-desc">${desc}</div></div>${control}</div>`;

app.innerHTML = `
  <header class="titlebar" id="titlebar" data-tauri-drag-region>
    ${logo}
    <h1>Settings</h1>
    <button class="icon-btn" id="close" aria-label="Close settings" title="Close">${icons.close}</button>
  </header>
  <div class="content" id="content">
    <div class="inner">
      <section id="overlay" aria-labelledby="h-overlay">
        <h2 id="h-overlay">Overlay</h2>
        <div class="group">
          ${item("5-hour limits", "Usage in the current rolling 5-hour window", sw("showFiveHour", "Show 5-hour limits"))}
          ${item("Weekly limits", "Usage in the current 7-day window", sw("showWeekly", "Show weekly limits"))}
        </div>
        <div class="group">
          ${item("Codex", '<span id="codexSummary"></span>', sw("showCodex", "Show Codex"))}
          ${item("Claude Code", '<span id="claudeSummary"></span>', sw("showClaude", "Show Claude Code"))}
          ${item("Antigravity", '<span id="antigravitySummary"></span>', sw("showAntigravity", "Show Antigravity"))}
        </div>
      </section>

      <section id="shortcut" aria-labelledby="h-shortcut">
        <h2 id="h-shortcut">Shortcut</h2>
        <div class="group">
          <div class="item">
            <div class="item-text">
              <div class="item-title">Show or hide Quota</div>
              <div class="item-desc" id="shortcutDesc">Works from any app. Click to record a new shortcut.</div>
            </div>
            <div class="recorder-wrap">
              <button class="recorder" id="recorder" aria-label="Change shortcut"></button>
              <button class="link" id="resetShortcut" hidden>Reset to default</button>
            </div>
          </div>
        </div>
      </section>

      <section id="providers" aria-labelledby="h-providers">
        <h2 id="h-providers">Providers</h2>
        <div class="group" id="codex">
          <div class="item stack">
            <div class="provider-head">
              <div class="item-title">Codex</div>
              <span class="status" id="codexStatus"></span>
            </div>
            <div class="item-desc">Read through the official <code>codex app-server</code> interface. Codex manages its own ChatGPT sign-in; Quota never reads it.</div>
            <div class="path" id="codexPath"></div>
            <form class="field" id="codexPathForm">
              <input id="codexPathInput" type="text" spellcheck="false" autocomplete="off" placeholder="Custom path to codex.exe (optional)" aria-label="Custom Codex path" />
              <button class="btn secondary" type="submit" id="codexPathSave">Save</button>
            </form>
          </div>
        </div>
        <div class="group" id="claude">
          <div class="item stack">
            <div class="provider-head">
              <div class="item-title">Claude Code</div>
              <span class="status" id="claudeStatus"></span>
            </div>
            <div class="item-desc">Anthropic doesn't offer an official API for subscription limits. Choose how Quota may read them.</div>
          </div>
          <div class="item stack">
            <div class="provider-head">
              <div class="item-text">
                <div class="item-title">Use Claude Code's sign-in <span class="tag accent">Recommended</span></div>
                <div class="item-desc">Asks Anthropic for your plan's 5-hour and weekly usage, anytime.</div>
              </div>
              ${sw("claudeEndpoint", "Use Claude Code's sign-in")}
            </div>
            <ul class="points">
              <li>${icons.info}<span>Uses an undocumented Anthropic endpoint, so it may stop working if Anthropic changes it.</span></li>
              <li>${icons.lock}<span>The sign-in Claude Code saved on this PC is read only when needed, sent only to <code>api.anthropic.com</code>, and never stored or logged by Quota.</span></li>
              <li>${icons.refresh}<span>Quota never renews the sign-in. If it expires, opening Claude Code renews it.</span></li>
            </ul>
          </div>
          <div class="item stack">
            <div class="provider-head">
              <div class="item-text">
                <div class="item-title">Status line bridge</div>
                <div class="item-desc" id="bridgeDesc">Uses the rate limits Claude Code reports to status line commands, Anthropic's documented interface. Updates only while you use Claude Code.</div>
              </div>
              ${sw("claudeBridge", "Status line bridge")}
            </div>
          </div>
        </div>
        <div class="group" id="antigravity">
          <div class="item stack">
            <div class="provider-head">
              <div class="item-text">
                <div class="item-title">Antigravity <span class="status" id="antigravityStatus"></span></div>
                <div class="item-desc">Reads the 5-hour and weekly limits for each model group from the Antigravity app while it's open.</div>
              </div>
              ${sw("antigravityApp", "Read quota from the Antigravity app")}
            </div>
            <ul class="points">
              <li>${icons.info}<span>Google doesn't offer an official way to read Antigravity quotas, so this uses the app's undocumented local interface and may stop working after an Antigravity update.</span></li>
              <li>${icons.lock}<span>Everything stays on this PC: Quota asks the running app over <code>127.0.0.1</code> with the per-session key the app passes to its own language server, and never stores or logs it.</span></li>
              <li>${icons.refresh}<span>When Antigravity is closed, Quota shows the last numbers it read.</span></li>
            </ul>
          </div>
        </div>
      </section>

      <section id="general" aria-labelledby="h-general">
        <h2 id="h-general">General</h2>
        <div class="group">
          ${item("Launch at startup", "Start Quota quietly when you sign in to Windows", sw("launchAtStartup", "Launch at startup"))}
          ${item("Hide when clicking outside", "Dismiss the overlay when it loses focus", sw("hideOnBlur", "Hide when clicking outside"))}
          ${item("Position", '<span id="positionDesc"></span>', '<button class="btn secondary" id="resetPosition">Reset</button>')}
          ${item(
            "Updates",
            '<span id="updatesDesc"></span>',
            `<div class="controls"><button class="btn secondary" id="updateAction">Check now</button>${sw("checkForUpdates", "Check for new versions automatically")}</div>`,
          )}
          ${item(
            "Appearance",
            "Follow Windows or pick a theme",
            `<div class="segmented" role="group" aria-label="Theme" id="theme">
              <button data-theme="system">System</button><button data-theme="light">Light</button><button data-theme="dark">Dark</button>
            </div>`,
          )}
          ${item(
            "Refresh",
            "Quota also checks for fresh usage whenever you open it",
            `<label class="select"><select id="refreshInterval" aria-label="Refresh interval">
              <option value="twoMinutes">Every 2 minutes</option>
              <option value="fiveMinutes">Every 5 minutes</option>
              <option value="fifteenMinutes">Every 15 minutes</option>
              <option value="onOpen">Only when opened</option>
            </select></label>`,
          )}
        </div>
      </section>

      <div class="footer">
        <div class="brandline">${logo}<span><strong>Quota</strong> <span id="version"></span> · Your AI usage. One shortcut away.</span></div>
        <button class="btn danger" id="quit">Quit Quota</button>
      </div>
    </div>
  </div>
  <div class="toast" id="toast" role="status" aria-live="polite"></div>`;

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

// ---------- Helpers ----------

let toastTimer = 0;
function toast(text: string, error = false) {
  const el = $("toast");
  el.textContent = text;
  el.className = `toast show${error ? " error" : ""}`;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.className = "toast"), error ? 4200 : 2200);
}

function setSwitch(id: string, on: boolean, disabled = false, title = "") {
  const el = $(id);
  el.setAttribute("aria-checked", String(on));
  el.toggleAttribute("disabled", disabled || busy.has(id));
  el.title = title;
}

function statusOf(p: ProviderState | undefined): { text: string; tone: "ok" | "warn" | "error" | "" } {
  if (!p) return { text: "", tone: "" };
  const s = p.status;
  switch (s.kind) {
    case "ready": {
      const plan = p.usage?.source === "claudeStatusLine" ? "Status line" : planLabel(p.usage?.plan);
      return { text: plan ? `Connected · ${plan}` : "Connected", tone: "ok" };
    }
    case "notInstalled":
      return { text: "Not installed", tone: "" };
    case "signedOut":
      return { text: "Not signed in", tone: "warn" };
    case "needsConsent":
      return { text: "Not connected", tone: "" };
    case "expired":
      return { text: "Sign-in expired", tone: "warn" };
    case "waiting":
      return { text: p.id === "antigravity" ? "App not open" : "Waiting for Claude Code", tone: "" };
    case "unsupported":
      return { text: "Unavailable", tone: "warn" };
    case "rateLimited":
      return { text: "Rate limited", tone: "warn" };
    case "error":
      return { text: "Can't connect", tone: "error" };
    default:
      return { text: p.refreshing ? "Checking…" : "Not checked yet", tone: "" };
  }
}

function setStatus(id: string, p: ProviderState | undefined) {
  const el = $(id);
  const { text, tone } = statusOf(p);
  el.textContent = text;
  el.className = `status ${tone}`;
  el.title = p && "message" in p.status ? p.status.message : "";
}

// ---------- Render ----------

function render() {
  if (!snapshot) return;
  const s = snapshot.settings;
  setTheme(s.theme);

  setSwitch("showFiveHour", s.showFiveHour, s.showFiveHour && !s.showWeekly, s.showFiveHour && !s.showWeekly ? "At least one limit type stays visible" : "");
  setSwitch("showWeekly", s.showWeekly, s.showWeekly && !s.showFiveHour, s.showWeekly && !s.showFiveHour ? "At least one limit type stays visible" : "");
  const shownProviders = [s.showCodex, s.showClaude, s.showAntigravity].filter(Boolean).length;
  const lastProvider = (on: boolean) => on && shownProviders === 1;
  for (const [id, on] of [["showCodex", s.showCodex], ["showClaude", s.showClaude], ["showAntigravity", s.showAntigravity]] as const) {
    setSwitch(id, on, lastProvider(on), lastProvider(on) ? "At least one provider stays visible" : "");
  }
  setSwitch("hideOnBlur", s.hideOnBlur);
  setSwitch("launchAtStartup", snapshot.launchAtStartup);

  const codex = snapshot.providers.find((p) => p.id === "codex");
  const claude = snapshot.providers.find((p) => p.id === "claude");
  $("codexSummary").textContent = statusOf(codex).text || "Subscription usage from your ChatGPT plan";
  $("claudeSummary").textContent = statusOf(claude).text || "Subscription usage from your Claude plan";
  const antigravity = snapshot.providers.find((p) => p.id === "antigravity");
  $("antigravitySummary").textContent = statusOf(antigravity).text || "Model quotas from the Antigravity app";
  setStatus("antigravityStatus", antigravity);
  setSwitch("antigravityApp", s.antigravityApp);
  setStatus("codexStatus", codex);
  setStatus("claudeStatus", claude);

  // Shortcut
  const rec = $("recorder");
  rec.classList.toggle("recording", recording);
  rec.innerHTML = recording
    ? `<span class="hint">Press a shortcut…</span>`
    : shortcutKeys(s.shortcut).map((k) => `<kbd>${escapeHtml(k)}</kbd>`).join("");
  $("resetShortcut").hidden = recording || s.shortcut === DEFAULT_SHORTCUT;
  const desc = $("shortcutDesc");
  const error = shortcutMessage?.error ? shortcutMessage.text : snapshot.shortcutError;
  desc.textContent = recording
    ? "Press Esc to cancel."
    : error ?? shortcutMessage?.text ?? "Works from any app. Click to record a new shortcut.";
  desc.classList.toggle("error", !recording && Boolean(error));

  // Codex
  $("codexPath").title = details?.codexPath ?? "";
  $("codexPath").textContent = details?.codexPath
    ? `${s.codexPath ? "Using" : "Found"} ${shortenPath(details.codexPath)}`
    : details
      ? "Codex wasn't found in the usual install locations."
      : "";
  const input = $<HTMLInputElement>("codexPathInput");
  if (document.activeElement !== input) input.value = s.codexPath ?? "";

  // Claude
  setSwitch("claudeEndpoint", s.claudeUsageEndpoint);
  const bridge = details?.claudeStatusLine.kind;
  const occupied = bridge === "occupied" && !s.claudeStatusLine;
  const unavailable = bridge === "unavailable" && !s.claudeStatusLine;
  setSwitch("claudeBridge", s.claudeStatusLine, occupied || unavailable);
  $("bridgeDesc").innerHTML = occupied
    ? "You already have a custom Claude Code status line, so Quota won't replace it."
    : unavailable
      ? "Install and sign in to Claude Code first."
      : s.claudeStatusLine
        ? "Quota is your Claude Code status line: it records the reported limits and shows the model and usage left. Turning this off restores the default."
        : "Uses the rate limits Claude Code reports to status line commands, Anthropic's documented interface. Updates only while you use Claude Code.";

  // General
  document.querySelectorAll<HTMLButtonElement>("#theme button").forEach((b) => {
    b.setAttribute("aria-pressed", String(b.dataset.theme === s.theme));
  });
  $<HTMLSelectElement>("refreshInterval").value = s.refreshInterval;
  $("positionDesc").textContent = s.overlayPosition
    ? "Quota opens where you last dragged it"
    : "Drag the overlay anywhere to move it; Quota remembers the spot";
  $<HTMLButtonElement>("resetPosition").disabled = !s.overlayPosition;
  renderUpdates();
  $("version").textContent = snapshot.version;
}

function renderUpdates() {
  if (!snapshot) return;
  const { update, version, settings: s } = snapshot;
  const desc = $("updatesDesc");
  const action = $<HTMLButtonElement>("updateAction");
  setSwitch("checkForUpdates", s.checkForUpdates);
  desc.classList.toggle("accent", Boolean(update.available));
  desc.classList.toggle("error", Boolean(update.error) && !update.available);
  action.textContent = update.available ? "Download" : "Check now";
  action.disabled = update.checking;
  if (update.checking) desc.textContent = "Checking for updates…";
  else if (update.available) desc.textContent = `Quota ${update.available.version} is available (you have ${version})`;
  else if (update.error) desc.textContent = update.error;
  else if (update.checkedAt) desc.textContent = `You have the latest version (${version}) · checked ${formatAgo(update.checkedAt, Date.now())}`;
  else if (s.checkForUpdates) desc.textContent = `Version ${version} · Quota looks for new versions every few hours`;
  else desc.textContent = `Version ${version} · automatic checks are off`;
}

async function loadDetails() {
  try {
    details = await api.providerDetails();
  } catch {
    details = null;
  }
  render();
}

// ---------- Actions ----------

// Saves run one at a time, and each sends every change made so far, so quick
// successive changes can't undo each other. State arriving while saves are
// still pending keeps those changes (see `adopt`).
let pendingSaves = 0;
let unsaved: Partial<Settings> = {};
let saveQueue: Promise<unknown> = Promise.resolve();

/** Takes new backend state without dropping changes that are still being saved. */
function adopt(next: AppSnapshot) {
  snapshot = pendingSaves ? { ...next, settings: { ...next.settings, ...unsaved } } : next;
}

/** Saves a change; resolves to whether it was saved. */
function saveSettings(patch: Partial<Settings>): Promise<boolean> {
  if (!snapshot) return Promise.resolve(false);
  unsaved = { ...unsaved, ...patch };
  snapshot = { ...snapshot, settings: { ...snapshot.settings, ...patch } }; // optimistic
  pendingSaves++;
  render();
  const save = saveQueue.then(async () => {
    let ok = true;
    let next: AppSnapshot | null = null;
    try {
      next = await api.updateSettings(snapshot!.settings);
    } catch (err) {
      ok = false;
      toast(errorText(err), true);
      next = await api.getState().catch(() => null);
    }
    pendingSaves--;
    if (!pendingSaves) unsaved = {};
    if (next) adopt(next);
    render();
    return ok;
  });
  saveQueue = save;
  return save;
}

async function withBusy(id: string, action: () => Promise<void>) {
  busy.add(id);
  render();
  try {
    await action();
  } catch (err) {
    toast(errorText(err), true);
  } finally {
    busy.delete(id);
    render();
  }
}

const toggles: Record<string, (on: boolean) => Promise<unknown>> = {
  showFiveHour: (on) => saveSettings({ showFiveHour: on }),
  showWeekly: (on) => saveSettings({ showWeekly: on }),
  showCodex: (on) => saveSettings({ showCodex: on }),
  showClaude: (on) => saveSettings({ showClaude: on }),
  hideOnBlur: (on) => saveSettings({ hideOnBlur: on }),
  launchAtStartup: (on) =>
    withBusy("launchAtStartup", async () => {
      const actual = await api.setLaunchAtStartup(on);
      if (snapshot) snapshot = { ...snapshot, launchAtStartup: actual };
    }),
  showAntigravity: (on) => saveSettings({ showAntigravity: on }),
  checkForUpdates: (on) => saveSettings({ checkForUpdates: on }),
  antigravityApp: (on) =>
    withBusy("antigravityApp", async () => {
      adopt(await api.setAntigravityApp(on));
      toast(on ? "Reading Antigravity quota from the app" : "Stopped reading Antigravity quota");
    }),
  claudeEndpoint: (on) =>
    withBusy("claudeEndpoint", async () => {
      adopt(await api.setClaudeUsageEndpoint(on));
      toast(on ? "Claude Code usage connected" : "Stopped reading Claude usage with your sign-in");
    }),
  claudeBridge: (on) =>
    withBusy("claudeBridge", async () => {
      adopt(await api.setClaudeStatusLine(on));
      await loadDetails();
      toast(on ? "Status line bridge installed" : "Status line bridge removed");
    }),
};

app.addEventListener("click", (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>(".switch");
  if (!el || el.hasAttribute("disabled")) return;
  const handler = toggles[el.id];
  if (handler) void handler(el.getAttribute("aria-checked") !== "true");
});

document.querySelectorAll<HTMLButtonElement>("#theme button").forEach((b) =>
  b.addEventListener("click", () => void saveSettings({ theme: b.dataset.theme as Theme })),
);

$<HTMLSelectElement>("refreshInterval").addEventListener("change", (e) =>
  void saveSettings({ refreshInterval: (e.target as HTMLSelectElement).value as RefreshInterval }),
);

$("updateAction").addEventListener("click", async () => {
  try {
    await (snapshot?.update.available ? api.openUpdatePage() : api.checkForUpdates());
  } catch (err) {
    toast(errorText(err), true);
  }
});

$("resetPosition").addEventListener("click", async () => {
  try {
    await api.resetOverlayPosition();
    toast("Overlay moved back to its default spot");
  } catch (err) {
    toast(errorText(err), true);
  }
});

$("codexPathForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  const value = $<HTMLInputElement>("codexPathInput").value.trim();
  if (!(await saveSettings({ codexPath: value || null }))) return;
  await loadDetails();
  toast(value ? "Codex path saved" : "Using automatic detection");
});

// Shortcut recording
async function startRecording() {
  if (recording) return;
  recording = true;
  shortcutMessage = null;
  await api.pauseShortcut(true).catch(() => {});
  render();
}

async function stopRecording() {
  recording = false;
  await api.pauseShortcut(false).catch(() => {});
  render();
}

async function applyShortcut(accelerator: string) {
  recording = false;
  try {
    const saved = await api.setShortcut(accelerator);
    shortcutMessage = { text: `Saved. Press ${shortcutKeys(saved).join(" + ")} to show or hide Quota.`, error: false };
    $("recorder").classList.add("saved");
    setTimeout(() => $("recorder").classList.remove("saved"), 1200);
  } catch (err) {
    shortcutMessage = { text: errorText(err), error: true };
    await api.pauseShortcut(false).catch(() => {});
  }
  adopt(await api.getState());
  render();
}

$("recorder").addEventListener("click", () => void startRecording());
$("recorder").addEventListener("blur", () => {
  if (recording) void stopRecording();
});
$("resetShortcut").addEventListener("click", () => void applyShortcut(DEFAULT_SHORTCUT));

window.addEventListener(
  "keydown",
  (e) => {
    if (!recording) {
      if (e.key === "Escape" && document.activeElement?.tagName !== "INPUT") void api.closeWindow();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      void stopRecording();
      return;
    }
    const result = recordKey(e);
    if (result.kind === "pending") {
      const rec = $("recorder");
      rec.innerHTML = result.modifiers.length
        ? result.modifiers.map((k) => `<kbd>${k}</kbd>`).join("") + `<span class="hint">+ …</span>`
        : `<span class="hint">Press a shortcut…</span>`;
    } else if (result.kind === "invalid") {
      shortcutMessage = { text: result.message, error: true };
      $("shortcutDesc").textContent = result.message;
      $("shortcutDesc").classList.add("error");
    } else {
      void applyShortcut(result.accelerator);
    }
  },
  true,
);

$("quit").addEventListener("click", () => void api.quit());
$("close").addEventListener("click", () => void api.closeWindow());

$("content").addEventListener("scroll", (e) => {
  $("titlebar").classList.toggle("scrolled", (e.target as HTMLElement).scrollTop > 2);
});

window.addEventListener("contextmenu", (e) => {
  if (!(e.target as HTMLElement).closest("input")) e.preventDefault();
});

function openSection(section: string) {
  const el = document.getElementById(section);
  if (!el) return;
  const target = el.tagName === "SECTION" ? el : el.closest("section")!;
  el.scrollIntoView({ block: "start" });
  target.classList.add("flash");
  setTimeout(() => target.classList.remove("flash"), 1200);
}

// ---------- Startup ----------

async function start() {
  try {
    await api.onState((s) => {
      adopt(s);
      render();
    });
    await api.on<string>("quota://open-section", (section) => openSection(section));
    snapshot = await api.getState();
    render();
    void loadDetails();
  } finally {
    // Always show the window, even if loading failed, so it never stays invisible.
    afterRender(async () => {
      await api.showWindow().catch(() => {});
      const section = location.hash.slice(1);
      if (section) setTimeout(() => openSection(section), 60);
    });
  }
}

void start();
