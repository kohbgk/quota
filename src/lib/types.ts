// Mirrors the Rust types serialized by the backend (src-tauri/src/model.rs, core.rs).

export type ProviderId = "codex" | "claude";
export type Theme = "system" | "light" | "dark";
export type RefreshInterval = "twoMinutes" | "fiveMinutes" | "fifteenMinutes" | "onOpen";

export interface LimitWindow {
  usedPercent: number;
  /** Unix seconds. */
  resetsAt: number | null;
  windowMinutes: number | null;
}

export type UsageSource = "codexAppServer" | "claudeUsageEndpoint" | "claudeStatusLine";

export interface Usage {
  fiveHour: LimitWindow | null;
  weekly: LimitWindow | null;
  plan: string | null;
  source: UsageSource;
  /** Unix milliseconds. */
  fetchedAt: number;
}

export type ProviderStatus =
  | { kind: "unknown" }
  | { kind: "ready" }
  | { kind: "notInstalled" }
  | { kind: "signedOut"; message: string }
  | { kind: "needsConsent" }
  | { kind: "expired" }
  | { kind: "waiting" }
  | { kind: "unsupported"; message: string }
  | { kind: "rateLimited"; retryAt: number | null }
  | { kind: "error"; message: string };

export interface ProviderState {
  id: ProviderId;
  status: ProviderStatus;
  usage: Usage | null;
  refreshing: boolean;
  lastAttemptAt: number | null;
}

export interface Settings {
  showFiveHour: boolean;
  showWeekly: boolean;
  showCodex: boolean;
  showClaude: boolean;
  shortcut: string;
  hideOnBlur: boolean;
  theme: Theme;
  refreshInterval: RefreshInterval;
  claudeUsageEndpoint: boolean;
  claudeStatusLine: boolean;
  codexPath: string | null;
  onboarded: boolean;
}

export interface AppSnapshot {
  settings: Settings;
  providers: ProviderState[];
  shortcutError: string | null;
  launchAtStartup: boolean;
  version: string;
  now: number;
  firstRun: boolean;
}

export type BridgeState =
  | { kind: "available" }
  | { kind: "installed" }
  | { kind: "occupied" }
  | { kind: "unavailable" };

export interface ProviderDetails {
  codexPath: string | null;
  claudeInstalled: boolean;
  claudeSignedIn: boolean;
  claudeStatusLine: BridgeState;
}
