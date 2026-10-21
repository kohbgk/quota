// Design-preview backend used only when the UI runs outside the desktop app
// (`npm run dev`, or a `--mode mock` build). Pick a scenario with `?scenario=`.
// The numbers below are placeholders for previews, never shown in the real app.

import type { Backend } from "../lib/api";
import type { AppSnapshot, ProviderDetails, ProviderState, Settings, Usage } from "../lib/types";

const now = () => Date.now();
const inSec = (ms: number) => Math.floor((now() + ms) / 1000);
const H = 3_600_000;

function usage(fiveLeft: number | null, weekLeft: number | null, plan: string | null, source: Usage["source"], ageMs = 20_000): Usage {
  return {
    fiveHour: fiveLeft === null ? null : { usedPercent: 100 - fiveLeft, resetsAt: inSec(2 * H + 14 * 60_000), windowMinutes: 300 },
    weekly: weekLeft === null ? null : { usedPercent: 100 - weekLeft, resetsAt: inSec(3 * 24 * H + 5 * H), windowMinutes: 10080 },
    plan,
    source,
    fetchedAt: now() - ageMs,
  };
}

const defaults: Settings = {
  showFiveHour: true,
  showWeekly: true,
  showCodex: true,
  showClaude: true,
  shortcut: "Ctrl+Shift+Space",
  hideOnBlur: true,
  theme: "dark",
  refreshInterval: "fiveMinutes",
  claudeUsageEndpoint: true,
  claudeStatusLine: false,
  codexPath: null,
  onboarded: true,
};

function provider(id: ProviderState["id"], status: ProviderState["status"], u: Usage | null, refreshing = false): ProviderState {
  return { id, status, usage: u, refreshing, lastAttemptAt: now() - 20_000 };
}

function scenario(name: string): AppSnapshot {
  const settings = { ...defaults };
  let codex = provider("codex", { kind: "ready" }, usage(72, 45, "plus", "codexAppServer"));
  let claude = provider("claude", { kind: "ready" }, usage(38, 81, "pro", "claudeUsageEndpoint", 40_000));
  let shortcutError: string | null = null;
  let firstRun = false;

  switch (name) {
    case "loading":
      codex = provider("codex", { kind: "unknown" }, null, true);
      claude = provider("claude", { kind: "unknown" }, null, true);
      break;
    case "codex-only":
      settings.showClaude = false;
      break;
    case "five-hour-only":
      settings.showWeekly = false;
      break;
    case "weekly-only":
      settings.showFiveHour = false;
      break;
    case "low":
      codex = provider("codex", { kind: "ready" }, usage(8, 24, "plus", "codexAppServer"));
      claude = provider("claude", { kind: "ready" }, usage(0, 52, "pro", "claudeUsageEndpoint"));
      claude.usage!.fiveHour!.resetsAt = inSec(37 * 60_000);
      break;
    case "stale":
      codex = provider("codex", { kind: "error", message: "Codex couldn't reach ChatGPT. Check your connection." }, usage(64, 40, "plus", "codexAppServer", 2 * H));
      claude = provider("claude", { kind: "rateLimited", retryAt: now() + 4 * 60_000 }, usage(55, 70, "pro", "claudeUsageEndpoint", 9 * 60_000));
      break;
    case "none":
      codex = provider("codex", { kind: "notInstalled" }, null);
      claude = provider("claude", { kind: "notInstalled" }, null);
      break;
    case "consent":
      claude = provider("claude", { kind: "needsConsent" }, null);
      settings.claudeUsageEndpoint = false;
      break;
    case "signedout":
      codex = provider("codex", { kind: "signedOut", message: "Run `codex login` to sign in with ChatGPT." }, null);
      claude = provider("claude", { kind: "expired" }, null);
      break;
    case "errors":
      codex = provider("codex", { kind: "unsupported", message: "Codex is signed in with an API key. Subscription limits are only available when signed in with ChatGPT." }, null);
      claude = provider("claude", { kind: "error", message: "Can't reach Anthropic. Check your connection." }, null);
      break;
    case "statusline":
      settings.claudeUsageEndpoint = false;
      settings.claudeStatusLine = true;
      claude = provider("claude", { kind: "ready" }, usage(61, 77, null, "claudeStatusLine", 3 * 60_000));
      break;
    case "waiting":
      settings.claudeUsageEndpoint = false;
      settings.claudeStatusLine = true;
      claude = provider("claude", { kind: "waiting" }, null);
      break;
    case "missing":
      codex = provider("codex", { kind: "ready" }, usage(null, 90, "free", "codexAppServer"));
      claude = provider("claude", { kind: "ready" }, usage(100, 81, "max", "claudeUsageEndpoint"));
      claude.usage!.fiveHour!.resetsAt = null;
      break;
    case "shortcut-error":
      shortcutError = "Ctrl+Shift+Space is already used by another app. Choose a different shortcut in Settings.";
      break;
    case "first-run":
      firstRun = true;
      break;
    case "light":
      settings.theme = "light";
      break;
    case "light-low":
      settings.theme = "light";
      codex = provider("codex", { kind: "ready" }, usage(8, 24, "plus", "codexAppServer"));
      claude = provider("claude", { kind: "rateLimited", retryAt: now() + 4 * 60_000 }, usage(55, 70, "pro", "claudeUsageEndpoint", 9 * 60_000));
      break;
  }
  return { settings, providers: [codex, claude], shortcutError, launchAtStartup: true, version: "0.1.0", now: now(), firstRun };
}

export function createMockBackend(): Backend {
  const params = new URLSearchParams(location.search);
  let state = scenario(params.get("scenario") ?? "both");
  const listeners = new Set<(s: AppSnapshot) => void>();
  const emit = () => {
    state = { ...state, now: now() };
    listeners.forEach((l) => l(structuredClone(state)));
  };
  const details: ProviderDetails = {
    codexPath: params.get("scenario") === "none" ? null : "C:\\Users\\you\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\vendor\\x86_64-pc-windows-msvc\\codex\\codex.exe",
    claudeInstalled: params.get("scenario") !== "none",
    claudeSignedIn: true,
    claudeStatusLine: { kind: params.get("bridge") === "occupied" ? "occupied" : state.settings.claudeStatusLine ? "installed" : "available" },
  };

  const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
    get_state: () => structuredClone(state),
    update_settings: ({ settings }) => {
      state = { ...state, settings: { ...(settings as Settings) } };
      emit();
      return structuredClone(state);
    },
    set_shortcut: ({ shortcut }) => {
      if (String(shortcut).includes("Alt+Space")) throw "Alt+Space is already used by another app. Try a different combination.";
      state.settings.shortcut = String(shortcut);
      emit();
      return shortcut;
    },
    pause_shortcut: () => undefined,
    set_launch_at_startup: ({ enabled }) => {
      state.launchAtStartup = Boolean(enabled);
      emit();
      return enabled;
    },
    refresh_now: () => {
      state.providers.forEach((p) => (p.refreshing = true));
      emit();
      setTimeout(() => {
        state.providers.forEach((p) => {
          p.refreshing = false;
          if (p.usage) p.usage.fetchedAt = now();
        });
        emit();
      }, 900);
    },
    set_claude_usage_endpoint: ({ enabled }) => {
      state.settings.claudeUsageEndpoint = Boolean(enabled);
      const claude = state.providers[1]!;
      Object.assign(claude, enabled ? { status: { kind: "ready" }, usage: usage(38, 81, "pro", "claudeUsageEndpoint") } : { status: { kind: "needsConsent" }, usage: null });
      emit();
      return structuredClone(state);
    },
    set_claude_status_line: ({ enabled }) => {
      state.settings.claudeStatusLine = Boolean(enabled);
      details.claudeStatusLine = { kind: enabled ? "installed" : "available" };
      emit();
      return structuredClone(state);
    },
    provider_details: () => structuredClone(details),
  };

  return {
    invoke: async <T>(cmd: string, args: Record<string, unknown> = {}) => {
      const handler = commands[cmd];
      return (handler ? handler(args) : undefined) as T;
    },
    listen: async <T>(event: string, handler: (payload: T) => void) => {
      if (event !== "quota://state") return () => {};
      const l = handler as unknown as (s: AppSnapshot) => void;
      listeners.add(l);
      return () => listeners.delete(l);
    },
    showWindow: async () => {},
    closeWindow: async () => {},
    startDragging: async () => {},
  };
}
