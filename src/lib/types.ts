// Mirrors the Rust types serialized by the backend (src-tauri/src/model.rs, core.rs).

export type ProviderId = "codex" | "claude" | "antigravity";
export type Theme = "system" | "light" | "dark";
export type RefreshInterval = "twoMinutes" | "fiveMinutes" | "fifteenMinutes" | "onOpen";

export interface LimitWindow {
  usedPercent: number;
  /** Unix seconds. */
  resetsAt: number | null;
  windowMinutes: number | null;
}

export type UsageSource = "codexAppServer" | "claudeUsageEndpoint" | "claudeStatusLine" | "antigravityApp";

/** Limits shared by a group of models (Antigravity: "Gemini", "Claude and GPT"). */
export interface QuotaGroup {
  name: string;
  fiveHour: LimitWindow | null;
  weekly: LimitWindow | null;
}

export interface Usage {
  fiveHour: LimitWindow | null;
  weekly: LimitWindow | null;
  plan: string | null;
  source: UsageSource;
  /** Unix milliseconds. */
  fetchedAt: number;
  /** The provider says included usage isn't allowed right now, whatever the percentages show. */
  blocked?: boolean;
  /** Per-model-group limits; when present, fiveHour and weekly are unused. */
  groups?: QuotaGroup[];
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
  showAntigravity: boolean;
  shortcut: string;
  hideOnBlur: boolean;
  theme: Theme;
  refreshInterval: RefreshInterval;
  claudeUsageEndpoint: boolean;
  claudeStatusLine: boolean;
  antigravityApp: boolean;
  codexPath: string | null;
  onboarded: boolean;
  /** Where the user dragged the overlay; null for the default spot. */
  overlayPosition: { x: number; y: number; monitor: string | null } | null;
  /** Look for new Quota releases and say when one is out. */
  checkForUpdates: boolean;
  notifiedUpdate: string | null;
  /** The version whose "update available" bar was closed. */
  dismissedUpdate: string | null;
}

export interface AppSnapshot {
  settings: Settings;
  providers: ProviderState[];
  shortcutError: string | null;
  launchAtStartup: boolean;
  version: string;
  now: number;
  firstRun: boolean;
  update: UpdateState;
}

export interface UpdateState {
  /** A newer release, with the page to download it from. */
  available: { version: string; url: string } | null;
  /** Unix milliseconds of the last completed check. */
  checkedAt: number | null;
  checking: boolean;
  error: string | null;
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
