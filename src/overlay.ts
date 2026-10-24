import "./styles/tokens.css";
import "./styles/overlay.css";

import { api, errorText } from "./lib/api";
import { escapeHtml, inlineCode, shortcutKeys } from "./lib/format";
import { afterRender } from "./lib/frame";
import { icons, logo } from "./lib/icons";
import { setTheme } from "./lib/theme";
import type { AppSnapshot } from "./lib/types";
import { buildOverlay, CardVM, MessageVM, RowVM } from "./lib/view";

const app = document.getElementById("app")!;
let snapshot: AppSnapshot | null = null;
let hintDismissed = false;
let actionError: string | null = null;

app.innerHTML = `
  <header class="top">
    <div class="brand">${logo}<span>Quota</span></div>
    <div class="top-actions">
      <button class="icon-btn" id="refresh" title="Refresh (R)" aria-label="Refresh usage">${icons.refresh}</button>
      <button class="icon-btn" id="settings" title="Settings (Ctrl+,)" aria-label="Open settings">${icons.sliders}</button>
    </div>
  </header>
  <div class="banners" id="banners"></div>
  <main class="cards" id="cards" aria-live="polite"></main>
  <footer class="foot">
    <span id="updated"></span>
    <span class="keys" id="keys" aria-label="Shortcut"></span>
  </footer>`;

const cardsEl = document.getElementById("cards")!;
const bannersEl = document.getElementById("banners")!;
const updatedEl = document.getElementById("updated")!;
const keysEl = document.getElementById("keys")!;
const refreshBtn = document.getElementById("refresh")!;

// ---------- Rendering ----------

function rowShape(row: RowVM): string {
  return `${row.key}:${row.kind}`;
}

function cardShape(card: CardVM): string {
  if (card.consent) return "consent";
  if (card.message) return `msg:${card.message.tone}:${card.message.title}:${card.message.action?.command ?? ""}`;
  return `rows:${card.rows.map(rowShape).join(",")}`;
}

function rowHtml(row: RowVM, provider: string): string {
  const label = `<span class="row-label">${row.label}</span>`;
  switch (row.kind) {
    case "value":
      return `
        <div class="row-top">${label}<span class="row-value"><b data-f="pct"></b>left</span></div>
        <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-label="${escapeHtml(provider)} ${row.label} usage remaining"><span class="bar-fill"></span></div>
        <div class="row-reset" data-f="reset"></div>`;
    case "loading":
      return `
        <div class="row-top">${label}<span class="row-value"><span class="skeleton"></span></span></div>
        <div class="bar"><span class="skeleton"></span></div>
        <div class="row-reset"><div class="skeleton"></div></div>`;
    default:
      return `
        <div class="row-top">${label}<span class="row-note" data-f="note"></span></div>
        <div class="bar"></div>`;
  }
}

const MESSAGE_ICONS: Record<string, string> = {
  neutral: icons.info,
  warn: icons.alert,
  error: icons.alert,
};

function messageHtml(card: CardVM, message: MessageVM): string {
  const icon =
    message.title.includes("isn't installed") ? icons.download
    : message.title === "Not signed in" ? icons.user
    : message.title.startsWith("Waiting") ? icons.clock
    : MESSAGE_ICONS[message.tone];
  const action = message.action
    ? `<button class="link" data-cmd="${message.action.command}" data-section="${message.action.section ?? ""}" data-provider="${card.id}">${escapeHtml(message.action.label)}</button>`
    : "";
  return `
    <div class="message tone-${message.tone}">
      <span class="msg-icon">${icon}</span>
      <div>
        <p class="msg-title">${escapeHtml(message.title)}</p>
        <p class="msg-body">${inlineCode(message.body)}</p>
        ${action}
      </div>
    </div>`;
}

function consentHtml(): string {
  return `
    <div class="consent">
      <p class="msg-title">Connect your Claude subscription</p>
      <p class="msg-body">Quota can read your Claude Code usage limits using the sign-in Claude Code already saved on this PC.</p>
      <p class="fine">${icons.lock}<span>Uses an undocumented Anthropic endpoint that may change. Your sign-in is sent only to Anthropic and is never stored by Quota.</span></p>
      <div class="actions">
        <button class="btn primary" data-cmd="consent">Allow</button>
        <button class="btn ghost" data-cmd="settings" data-section="claude">Other options</button>
      </div>
    </div>`;
}

function buildCard(el: HTMLElement, card: CardVM) {
  let body = "";
  if (card.consent) body = consentHtml();
  else if (card.message) body = messageHtml(card, card.message);
  else
    body = `<div class="rows">${card.rows
      .map((r) => `<div class="row" data-key="${r.key}">${rowHtml(r, card.name)}</div>`)
      .join("")}</div>`;
  el.innerHTML = `
    <header class="card-head">
      <h2 class="card-name">${card.name}</h2>
      <span class="card-spinner" aria-hidden="true"></span>
      <span class="badge" data-f="badge"></span>
    </header>
    <p class="notice" data-f="notice"></p>
    ${body}`;
  el.dataset.shape = cardShape(card);
}

function fillRow(el: HTMLElement, row: RowVM) {
  el.className = "row";
  if (row.kind === "value") {
    el.classList.add(row.level);
    el.querySelector<HTMLElement>('[data-f="pct"]')!.textContent = row.display;
    const bar = el.querySelector<HTMLElement>(".bar")!;
    bar.setAttribute("aria-valuenow", String(row.remaining));
    bar.setAttribute("aria-valuetext", `${row.display} left`);
    const fill = el.querySelector<HTMLElement>(".bar-fill")!;
    // Let the first paint happen at 0 so new bars animate in.
    requestAnimationFrame(() => (fill.style.width = `${row.remaining}%`));
    const reset = el.querySelector<HTMLElement>('[data-f="reset"]')!;
    reset.textContent = row.reset;
    reset.title = row.resetTitle;
  } else if (row.kind === "loading") {
    el.classList.add("loading");
  } else {
    el.classList.add("muted");
    el.querySelector<HTMLElement>('[data-f="note"]')!.textContent = row.note;
  }
}

function fillCard(el: HTMLElement, card: CardVM) {
  el.classList.toggle("refreshing", card.refreshing);
  el.classList.toggle("dim", card.dim);
  const badge = el.querySelector<HTMLElement>('[data-f="badge"]')!;
  badge.textContent = card.badge ?? "";
  badge.title = card.badgeTitle ?? "";
  const notice = el.querySelector<HTMLElement>('[data-f="notice"]')!;
  notice.textContent = card.notice?.text ?? "";
  notice.className = `notice ${card.notice?.tone ?? ""}`;
  for (const row of card.rows) {
    const rowEl = el.querySelector<HTMLElement>(`.row[data-key="${row.key}"]`);
    if (rowEl) fillRow(rowEl, row);
  }
  // The card's structure is rebuilt only when its shape changes; keep message text
  // (error details, retry countdowns) current in between.
  const body = el.querySelector<HTMLElement>(".msg-body");
  if (body && card.message && body.dataset.text !== card.message.body) {
    body.dataset.text = card.message.body;
    body.innerHTML = inlineCode(card.message.body);
  }
}

function renderBanners(s: AppSnapshot) {
  const parts: string[] = [];
  if (s.shortcutError) {
    parts.push(`
      <div class="banner warn" role="alert">${icons.alert}
        <p>${escapeHtml(s.shortcutError.replace(" Choose a different shortcut in Settings.", ""))}</p>
        <button class="link" data-cmd="settings" data-section="shortcut">Change</button>
      </div>`);
  } else if (s.firstRun && !hintDismissed) {
    parts.push(`
      <div class="banner">${icons.keyboard}
        <p>Quota keeps running in your tray. Use the shortcut below anytime.</p>
        <button class="link" data-cmd="dismiss-hint">Got it</button>
      </div>`);
  }
  if (actionError) {
    parts.push(`<div class="banner warn" role="alert">${icons.alert}<p>${escapeHtml(actionError)}</p></div>`);
  }
  const html = parts.join("");
  if (bannersEl.dataset.html !== html) {
    bannersEl.innerHTML = html;
    bannersEl.dataset.html = html;
  }
}

function render() {
  if (!snapshot) return;
  const now = Date.now();
  setTheme(snapshot.settings.theme);
  const vm = buildOverlay(snapshot, now);

  renderBanners(snapshot);

  // Keyed card reconciliation keeps bars animating smoothly between updates.
  const existing = new Map([...cardsEl.children].map((el) => [(el as HTMLElement).dataset.id!, el as HTMLElement]));
  vm.cards.forEach((card, index) => {
    let el = existing.get(card.id);
    if (!el) {
      el = document.createElement("section");
      el.className = "card";
      el.dataset.id = card.id;
      el.setAttribute("aria-label", card.name);
    }
    if (el.dataset.shape !== cardShape(card)) buildCard(el, card);
    fillCard(el, card);
    if (cardsEl.children[index] !== el) cardsEl.insertBefore(el, cardsEl.children[index] ?? null);
    existing.delete(card.id);
  });
  existing.forEach((el) => el.remove());

  refreshBtn.classList.toggle("spinning", vm.refreshing);
  updatedEl.textContent = vm.updated;
  const keys = shortcutKeys(snapshot.settings.shortcut);
  const keysHtml = keys.map((k) => `<kbd>${escapeHtml(k)}</kbd>`).join("");
  if (keysEl.dataset.html !== keysHtml) {
    keysEl.innerHTML = keysHtml;
    keysEl.dataset.html = keysHtml;
    keysEl.title = `Press ${keys.join(" + ")} to show or hide Quota`;
  }
}

// ---------- Sizing ----------

let lastHeight = 0;
new ResizeObserver(() => {
  const height = Math.ceil(app.getBoundingClientRect().height);
  if (height > 0 && height !== lastHeight) {
    lastHeight = height;
    api.resizeOverlay(height).catch(() => {});
  }
}).observe(app);

// ---------- Interaction ----------

async function run(action: () => Promise<unknown>) {
  try {
    actionError = null;
    await action();
  } catch (err) {
    actionError = errorText(err);
  }
  render();
}

app.addEventListener("click", (e) => {
  const target = (e.target as HTMLElement).closest<HTMLElement>("[data-cmd]");
  if (!target) return;
  const { cmd, section, provider } = target.dataset;
  switch (cmd) {
    case "retry":
      void run(() => api.refresh(provider as never));
      break;
    case "consent":
      void run(() => api.setClaudeUsageEndpoint(true));
      break;
    case "settings":
      void run(() => api.openSettings(section || undefined));
      break;
    case "dismiss-hint":
      hintDismissed = true;
      render();
      break;
  }
});

// Drag the overlay from anywhere that isn't a control; Quota remembers where it's dropped.
// The move starts only once the mouse has moved with the button held, so plain clicks stay clicks.
let pressedAt: { x: number; y: number } | null = null;
app.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button, a, input, select, [data-cmd]")) return;
  e.preventDefault();
  pressedAt = { x: e.screenX, y: e.screenY };
});
window.addEventListener("mousemove", (e) => {
  if (!pressedAt) return;
  if (!(e.buttons & 1)) {
    pressedAt = null;
  } else if (Math.hypot(e.screenX - pressedAt.x, e.screenY - pressedAt.y) >= 4) {
    pressedAt = null;
    void api.startOverlayDrag().catch(() => {});
  }
});
window.addEventListener("mouseup", () => (pressedAt = null));

refreshBtn.addEventListener("click", () => void run(() => api.refresh()));
document.getElementById("settings")!.addEventListener("click", () => void run(() => api.openSettings()));

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    void api.hideOverlay();
  } else if ((e.key === "r" || e.key === "R" || e.key === "F5") && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault();
    void run(() => api.refresh());
  } else if (e.key === "," && e.ctrlKey) {
    e.preventDefault();
    void run(() => api.openSettings());
  }
});

// No browser context menu in a desktop overlay.
window.addEventListener("contextmenu", (e) => e.preventDefault());

// Keep countdowns and "updated" labels current from local time.
setInterval(render, 10_000);

// ---------- Startup ----------

async function start() {
  await api.onState((s) => {
    snapshot = s;
    render();
  });
  await api.on("quota://overlay-shown", () => {
    actionError = null;
    render();
    app.focus();
  });
  snapshot = await api.getState();
  render();
  // Tell the backend we've rendered once, so the first show never flashes empty.
  afterRender(() => void api.overlayReady().catch(() => {}));
}

void start();
