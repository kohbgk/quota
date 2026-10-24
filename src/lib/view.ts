// Turns backend state into a display model for the overlay. Pure and testable:
// every decision about what the user sees (fresh, stale, unavailable…) lives here.

import { formatAgo, formatIn, formatReset, formatResetTitle, Level, levelFor, planLabel, remainingPercent } from "./format";
import type { AppSnapshot, LimitWindow, ProviderId, ProviderState, Settings, Usage } from "./types";

export type WindowKey = "fiveHour" | "weekly";

export type RowVM =
  | { key: WindowKey; label: string; kind: "value"; remaining: number; level: Level; reset: string; resetTitle: string }
  | { key: WindowKey; label: string; kind: "missing"; note: string }
  | { key: WindowKey; label: string; kind: "reset"; note: string }
  | { key: WindowKey; label: string; kind: "loading" };

export interface MessageVM {
  tone: "neutral" | "warn" | "error";
  title: string;
  body: string;
  action?: { label: string; command: "retry" | "consent" | "settings"; section?: string };
}

export interface CardVM {
  id: ProviderId;
  name: string;
  badge: string | null;
  badgeTitle?: string;
  refreshing: boolean;
  /** Present when data is shown but is not current. */
  notice: { tone: "muted" | "warn"; text: string } | null;
  dim: boolean;
  rows: RowVM[];
  message: MessageVM | null;
  consent: boolean;
}

export interface OverlayVM {
  cards: CardVM[];
  updated: string;
  refreshing: boolean;
}

const NAMES: Record<ProviderId, string> = { codex: "Codex", claude: "Claude Code" };
const LABELS: Record<WindowKey, string> = { fiveHour: "5-hour", weekly: "Weekly" };

export function visibleWindows(settings: Settings): WindowKey[] {
  const keys: WindowKey[] = [];
  if (settings.showFiveHour) keys.push("fiveHour");
  if (settings.showWeekly) keys.push("weekly");
  return keys;
}

export function visibleProviders(snapshot: AppSnapshot): ProviderState[] {
  const s = snapshot.settings;
  return snapshot.providers.filter((p) => (p.id === "codex" ? s.showCodex : s.showClaude));
}

/** Data older than this is labelled as out of date even without an error. */
export function staleAfterMs(settings: Settings): number {
  const interval = { twoMinutes: 2, fiveMinutes: 5, fifteenMinutes: 15, onOpen: 5 }[settings.refreshInterval];
  return Math.max(15, interval * 3) * 60_000;
}

function rowFor(key: WindowKey, window: LimitWindow | null, id: ProviderId, now: number): RowVM {
  const label = LABELS[key];
  if (!window) {
    return {
      key,
      label,
      kind: "missing",
      note: id === "codex" && key === "fiveHour" ? "Not reported for this plan" : "Not reported",
    };
  }
  // Past its reset time, a window's numbers are out of date however recently they were fetched
  // (Claude Code's status line stops reporting a window once it resets).
  if (window.resetsAt !== null && window.resetsAt * 1000 <= now) {
    return { key, label, kind: "reset", note: "Reset since last update" };
  }
  const remaining = remainingPercent(window.usedPercent);
  let reset: string;
  let resetTitle = "";
  if (window.resetsAt !== null) {
    reset = formatReset(window.resetsAt, now);
    resetTitle = formatResetTitle(window.resetsAt, now);
  } else if (remaining === 100) {
    reset = "Starts with your next request";
  } else {
    reset = "Reset time not reported";
  }
  if (remaining === 0) reset = `Limit reached · ${reset[0]!.toLowerCase()}${reset.slice(1)}`;
  return { key, label, kind: "value", remaining, level: levelFor(remaining), reset, resetTitle };
}

function sourceBadge(usage: Usage | null): { badge: string | null; title?: string } {
  if (!usage) return { badge: null };
  if (usage.source === "claudeStatusLine") {
    return { badge: "Status line", title: "Reported by Claude Code's status line" };
  }
  return { badge: planLabel(usage.plan) };
}

function messageFor(p: ProviderState): { message: MessageVM | null; consent: boolean } {
  const name = NAMES[p.id];
  const s = p.status;
  switch (s.kind) {
    case "notInstalled":
      return {
        consent: false,
        message: {
          tone: "neutral",
          title: `${name} isn't installed`,
          body:
            p.id === "codex"
              ? "Install the Codex CLI and sign in with ChatGPT. Quota will find it automatically."
              : "Install Claude Code and sign in with your Claude subscription.",
          action: p.id === "codex" ? { label: "Set path…", command: "settings", section: "providers" } : undefined,
        },
      };
    case "signedOut":
      return { consent: false, message: { tone: "neutral", title: "Not signed in", body: s.message } };
    case "needsConsent":
      return { consent: true, message: null };
    case "unsupported":
      return { consent: false, message: { tone: "neutral", title: "Usage isn't available", body: s.message } };
    case "expired":
      return {
        consent: false,
        message: {
          tone: "warn",
          title: "Claude Code sign-in expired",
          body: "Open Claude Code to renew it. Quota picks it up automatically.",
        },
      };
    case "waiting":
      return {
        consent: false,
        message: {
          tone: "neutral",
          title: "Waiting for Claude Code",
          body: "Usage appears after your next message in Claude Code.",
        },
      };
    case "rateLimited":
      return {
        consent: false,
        message: {
          tone: "warn",
          title: "Anthropic is limiting requests",
          body: `Quota will try again ${s.retryAt ? formatIn(s.retryAt, Date.now()) : "shortly"}.`,
        },
      };
    case "error":
      return {
        consent: false,
        message: { tone: "error", title: "Couldn't load usage", body: s.message, action: { label: "Try again", command: "retry" } },
      };
    default:
      return { consent: false, message: null };
  }
}

/** Statuses where previously retrieved numbers are still worth showing (dimmed). */
const KEEP_DATA = new Set(["ready", "unknown", "error", "rateLimited", "expired", "waiting"]);

export function cardFor(p: ProviderState, snapshot: AppSnapshot, now: number): CardVM {
  const windows = visibleWindows(snapshot.settings);
  const base = {
    id: p.id,
    name: NAMES[p.id],
    refreshing: p.refreshing,
    notice: null,
    dim: false,
    rows: [] as RowVM[],
    message: null,
    consent: false,
  } satisfies Partial<CardVM>;

  const usage = p.usage && KEEP_DATA.has(p.status.kind) ? p.usage : null;

  if (!usage) {
    // First load: placeholders shaped like the final rows, so nothing jumps.
    if (p.status.kind === "unknown" || p.status.kind === "ready") {
      return { ...base, badge: null, rows: windows.map((key) => ({ key, label: LABELS[key], kind: "loading" as const })) };
    }
    const { message, consent } = messageFor(p);
    return { ...base, badge: null, message, consent };
  }

  const { badge, title } = sourceBadge(usage);
  const rows = windows.map((key) => rowFor(key, usage[key], p.id, now));
  const age = now - usage.fetchedAt;
  let notice: CardVM["notice"] = null;
  let dim = false;
  switch (p.status.kind) {
    case "error":
      notice = { tone: "warn", text: `Couldn't refresh · showing data from ${formatAgo(usage.fetchedAt, now)}` };
      dim = true;
      break;
    case "rateLimited":
      notice = {
        tone: "warn",
        text: `Rate limited · retrying ${p.status.retryAt ? formatIn(p.status.retryAt, now) : "shortly"}`,
      };
      dim = age > staleAfterMs(snapshot.settings);
      break;
    case "expired":
      notice = { tone: "warn", text: `Sign-in expired · data from ${formatAgo(usage.fetchedAt, now)}` };
      dim = true;
      break;
    default:
      if (age > staleAfterMs(snapshot.settings)) {
        notice = { tone: "muted", text: `Last updated ${formatAgo(usage.fetchedAt, now)}` };
        dim = true;
      }
  }
  // Whatever else the card says, never let the percentages imply blocked usage is available.
  if (usage.blocked) {
    notice = notice
      ? { tone: "warn", text: `${notice.text} · included usage unavailable` }
      : { tone: "warn", text: "Included usage isn't available right now" };
  }
  return { ...base, badge, badgeTitle: title, rows, notice, dim };
}

export function buildOverlay(snapshot: AppSnapshot, now: number): OverlayVM {
  const providers = visibleProviders(snapshot);
  const cards = providers.map((p) => cardFor(p, snapshot, now));
  const refreshing = providers.some((p) => p.refreshing);
  const fetchedAt = (pred: (p: ProviderState) => boolean) =>
    providers.filter((p) => p.usage && pred(p)).map((p) => p.usage!.fetchedAt);
  // Prefer current data; otherwise report the oldest data still on screen.
  let times = fetchedAt((p) => p.status.kind === "ready");
  if (!times.length) times = fetchedAt((p) => KEEP_DATA.has(p.status.kind));
  let updated = "";
  if (refreshing) updated = "Refreshing…";
  else if (times.length) updated = `Updated ${formatAgo(Math.min(...times), now)}`;
  return { cards, updated, refreshing };
}
