// Thin, typed wrapper over Tauri commands and events.
// When the page runs outside Tauri (design previews), a mock backend is used.

import type { AppSnapshot, ProviderDetails, ProviderId, Settings } from "./types";

export const STATE_EVENT = "quota://state";

type Unlisten = () => void;

export interface Backend {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten>;
  showWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  startDragging(): Promise<void>;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let backendPromise: Promise<Backend> | null = null;

function backend(): Promise<Backend> {
  backendPromise ??= inTauri
    ? (async () => {
        const [{ invoke }, { listen }, { getCurrentWindow }] = await Promise.all([
          import("@tauri-apps/api/core"),
          import("@tauri-apps/api/event"),
          import("@tauri-apps/api/window"),
        ]);
        return {
          invoke: (cmd, args) => invoke(cmd, args),
          listen: async (event, handler) => listen(event, (e) => handler(e.payload as never)),
          showWindow: async () => {
            const w = getCurrentWindow();
            await w.show();
            await w.setFocus();
          },
          closeWindow: () => getCurrentWindow().close(),
          startDragging: () => getCurrentWindow().startDragging(),
        } satisfies Backend;
      })()
    : import.meta.env.MODE === "mock" || import.meta.env.DEV
      ? import("../dev/mock").then((m) => m.createMockBackend())
      : Promise.reject(new Error("Quota must run inside its desktop app."));
  return backendPromise;
}

const call = async <T>(cmd: string, args?: Record<string, unknown>) => (await backend()).invoke<T>(cmd, args);

export const api = {
  getState: () => call<AppSnapshot>("get_state"),
  updateSettings: (settings: Settings) => call<AppSnapshot>("update_settings", { settings }),
  setShortcut: (shortcut: string) => call<string>("set_shortcut", { shortcut }),
  pauseShortcut: (paused: boolean) => call<void>("pause_shortcut", { paused }),
  setLaunchAtStartup: (enabled: boolean) => call<boolean>("set_launch_at_startup", { enabled }),
  refresh: (provider?: ProviderId) => call<void>("refresh_now", { provider: provider ?? null }),
  hideOverlay: () => call<void>("hide_overlay"),
  overlayReady: () => call<void>("overlay_ready"),
  resizeOverlay: (height: number) => call<void>("resize_overlay", { height }),
  openSettings: (section?: string) => call<void>("open_settings", { section: section ?? null }),
  quit: () => call<void>("quit_app"),
  setClaudeUsageEndpoint: (enabled: boolean) => call<AppSnapshot>("set_claude_usage_endpoint", { enabled }),
  setClaudeStatusLine: (enabled: boolean) => call<AppSnapshot>("set_claude_status_line", { enabled }),
  providerDetails: () => call<ProviderDetails>("provider_details"),

  onState: async (handler: (s: AppSnapshot) => void) => (await backend()).listen(STATE_EVENT, handler),
  on: async <T>(event: string, handler: (payload: T) => void) => (await backend()).listen(event, handler),
  showWindow: async () => (await backend()).showWindow(),
  closeWindow: async () => (await backend()).closeWindow(),
  startDragging: async () => (await backend()).startDragging(),
};

/** Error text from a rejected command. */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong.";
}
