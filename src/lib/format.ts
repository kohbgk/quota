// Pure formatting helpers. All times are computed locally from absolute timestamps.

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Percentage left, rounded down so Quota never overstates what remains. */
export function remainingPercent(usedPercent: number): number {
  const used = Number.isFinite(usedPercent) ? Math.min(100, Math.max(0, usedPercent)) : 0;
  return Math.max(0, Math.min(100, Math.floor(100 - used + 1e-9)));
}

export type Level = "ok" | "warn" | "low";

export function levelFor(remaining: number): Level {
  if (remaining <= 10) return "low";
  if (remaining <= 30) return "warn";
  return "ok";
}

/** "4h 12m", "42m", "<1m", "3d 4h". */
export function formatDuration(ms: number): string {
  if (ms < MINUTE) return "<1m";
  if (ms < HOUR) return `${Math.floor(ms / MINUTE)}m`;
  if (ms < DAY) {
    const h = Math.floor(ms / HOUR);
    const m = Math.floor((ms % HOUR) / MINUTE);
    return m ? `${h}h ${m}m` : `${h}h`;
  }
  const d = Math.floor(ms / DAY);
  const h = Math.floor((ms % DAY) / HOUR);
  return h ? `${d}d ${h}h` : `${d}d`;
}

const timeFmt = () => new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const weekdayFmt = () => new Intl.DateTimeFormat(undefined, { weekday: "short" });
const dateFmt = () => new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const fullFmt = () =>
  new Intl.DateTimeFormat(undefined, { weekday: "long", month: "long", day: "numeric", hour: "numeric", minute: "2-digit" });

/**
 * Short reset label. Within a day it counts down ("Resets in 2h 14m");
 * further out it names the local day and time ("Resets Thu 9:00 AM").
 */
export function formatReset(resetsAtSec: number, now: number): string {
  const at = resetsAtSec * 1000;
  const diff = at - now;
  if (diff <= 0) return "Resetting now";
  if (diff < DAY) return `Resets in ${formatDuration(diff)}`;
  const date = new Date(at);
  if (diff < 6 * DAY) return `Resets ${weekdayFmt().format(date)} ${timeFmt().format(date)}`;
  return `Resets ${dateFmt().format(date)}`;
}

/** Full local date for tooltips. */
export function formatResetTitle(resetsAtSec: number, now: number): string {
  const at = resetsAtSec * 1000;
  const when = fullFmt().format(new Date(at));
  const diff = at - now;
  return diff > 0 ? `${when} (in ${formatDuration(diff)})` : when;
}

/** "just now", "4m ago", "2h ago", "3d ago". */
export function formatAgo(timestamp: number, now: number): string {
  const diff = Math.max(0, now - timestamp);
  if (diff < 45_000) return "just now";
  if (diff < HOUR) return `${Math.max(1, Math.round(diff / MINUTE))}m ago`;
  if (diff < DAY) return `${Math.floor(diff / HOUR)}h ago`;
  return `${Math.floor(diff / DAY)}d ago`;
}

/** "in 4m" style for retry times. */
export function formatIn(timestamp: number, now: number): string {
  const diff = timestamp - now;
  return diff <= 0 ? "shortly" : `in ${formatDuration(diff)}`;
}

const PLAN_NAMES: Record<string, string> = {
  free: "Free",
  go: "Go",
  plus: "Plus",
  pro: "Pro",
  max: "Max",
  team: "Team",
  business: "Business",
  enterprise: "Enterprise",
  edu: "Edu",
};

export function planLabel(plan: string | null | undefined): string | null {
  if (!plan) return null;
  const key = plan.trim().toLowerCase();
  if (!key || key === "unknown") return null;
  return (
    PLAN_NAMES[key] ??
    key
      .split(/[_\s-]+/)
      .filter(Boolean)
      .map((w) => w[0]!.toUpperCase() + w.slice(1))
      .join(" ")
  );
}

/** Splits a canonical accelerator ("Ctrl+Shift+Space") into keycap labels. */
export function shortcutKeys(shortcut: string): string[] {
  const names: Record<string, string> = {
    ArrowUp: "↑",
    ArrowDown: "↓",
    ArrowLeft: "←",
    ArrowRight: "→",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    Comma: ",",
    Period: ".",
    Slash: "/",
    PageUp: "PgUp",
    PageDown: "PgDn",
    Delete: "Del",
    Insert: "Ins",
  };
  return shortcut
    .split("+")
    .filter(Boolean)
    .map((k) => names[k] ?? k.replace(/^Numpad/, "Num "));
}

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

/** Renders `code` spans for backtick-quoted text in trusted, escaped messages. */
export function inlineCode(text: string): string {
  return escapeHtml(text).replace(/`([^`]+)`/g, "<code>$1</code>");
}
