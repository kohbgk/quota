import { describe, expect, it } from "vitest";
import type { AppSnapshot, ProviderState, Settings, Usage } from "./types";
import { buildOverlay } from "./view";

const NOW = Date.UTC(2026, 9, 21, 12, 0, 0);
const sec = (ms: number) => Math.floor((NOW + ms) / 1000);

const settings: Settings = {
  showFiveHour: true,
  showWeekly: true,
  showCodex: true,
  showClaude: true,
  showAntigravity: true,
  shortcut: "Ctrl+Shift+Space",
  hideOnBlur: true,
  theme: "system",
  refreshInterval: "fiveMinutes",
  claudeUsageEndpoint: true,
  claudeStatusLine: false,
  antigravityApp: true,
  checkForUpdates: true,
  notifiedUpdate: null,
  dismissedUpdate: null,
  settingsVersion: 1,
  codexPath: null,
  onboarded: true,
  overlayPosition: null,
};

function usage(fiveUsed: number | null, weekUsed: number | null, fetchedAgo = 10_000): Usage {
  return {
    fiveHour: fiveUsed === null ? null : { usedPercent: fiveUsed, resetsAt: sec(3_600_000), windowMinutes: 300 },
    weekly: weekUsed === null ? null : { usedPercent: weekUsed, resetsAt: sec(86_400_000 * 3), windowMinutes: 10080 },
    plan: "plus",
    source: "codexAppServer",
    fetchedAt: NOW - fetchedAgo,
  };
}

function p(id: ProviderState["id"], status: ProviderState["status"], u: Usage | null): ProviderState {
  return { id, status, usage: u, refreshing: false, lastAttemptAt: NOW };
}

function snap(providers: ProviderState[], overrides: Partial<Settings> = {}): AppSnapshot {
  return { settings: { ...settings, ...overrides }, providers, shortcutError: null, launchAtStartup: true, version: "0", now: NOW, firstRun: false, update: { available: null, checkedAt: null, checking: false, error: null } };
}

const ready = () => [p("codex", { kind: "ready" }, usage(28, 55)), p("claude", { kind: "ready" }, usage(62, 19))];

describe("overlay view model", () => {
  it("shows remaining percentages for both providers and both windows", () => {
    const vm = buildOverlay(snap(ready()), NOW);
    expect(vm.cards.map((c) => c.name)).toEqual(["Codex", "Claude Code"]);
    const rows = vm.cards[0]!.rows;
    expect(rows.map((r) => r.key)).toEqual(["fiveHour", "weekly"]);
    expect(rows[0]).toMatchObject({ kind: "value", remaining: 72, reset: "Resets in 1h" });
    expect(vm.cards[1]!.rows[0]).toMatchObject({ kind: "value", remaining: 38 });
    expect(vm.cards[0]!.badge).toBe("Plus");
    expect(vm.updated).toBe("Updated just now");
  });

  it("drops weekly rows when weekly limits are hidden", () => {
    const vm = buildOverlay(snap(ready(), { showWeekly: false }), NOW);
    expect(vm.cards.every((c) => c.rows.length === 1 && c.rows[0]!.key === "fiveHour")).toBe(true);
  });

  it("shows only weekly rows when 5-hour limits are hidden", () => {
    const vm = buildOverlay(snap(ready(), { showFiveHour: false }), NOW);
    expect(vm.cards.every((c) => c.rows.map((r) => r.key).join() === "weekly")).toBe(true);
  });

  it("hides providers that are turned off", () => {
    expect(buildOverlay(snap(ready(), { showCodex: false }), NOW).cards.map((c) => c.id)).toEqual(["claude"]);
    expect(buildOverlay(snap(ready(), { showClaude: false }), NOW).cards.map((c) => c.id)).toEqual(["codex"]);
  });

  it("never invents a limit the provider didn't report", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, usage(null, 10)), ready()[1]!]), NOW);
    expect(vm.cards[0]!.rows[0]).toMatchObject({ kind: "missing" });
    expect(vm.cards[0]!.rows[1]).toMatchObject({ kind: "value", remaining: 90 });
  });

  it("marks windows that reset since the last fetch instead of showing old numbers", () => {
    const u = usage(90, 20, 2 * 3_600_000);
    u.fiveHour!.resetsAt = sec(-60_000);
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, u), ready()[1]!]), NOW);
    expect(vm.cards[0]!.rows[0]).toMatchObject({ kind: "reset" });
  });

  it("treats a window past its reset time as reset even when recently fetched", () => {
    // Claude Code's status line stops sending a window once it resets; Quota keeps the old one.
    const u = { ...usage(90, 20, 10_000), plan: null, source: "claudeStatusLine" as const };
    u.fiveHour!.resetsAt = sec(-60_000);
    const vm = buildOverlay(snap([ready()[0]!, p("claude", { kind: "ready" }, u)]), NOW);
    expect(vm.cards[1]!.rows[0]).toMatchObject({ kind: "reset", note: "Reset since last update" });
    expect(vm.cards[1]!.rows[1]).toMatchObject({ kind: "value", remaining: 80 });
  });

  it("warns when the provider says included usage is blocked", () => {
    const u = { ...usage(40, 20), blocked: true };
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, u), ready()[1]!]), NOW);
    expect(vm.cards[0]!.notice).toEqual({ tone: "warn", text: "Included usage isn't available right now" });
    expect(vm.cards[1]!.notice).toBeNull();

    // Still said when the data is also old or couldn't be refreshed.
    const old = { ...usage(40, 20, 3_600_000), blocked: true };
    const stale = buildOverlay(snap([p("codex", { kind: "error", message: "offline" }, old), ready()[1]!]), NOW);
    expect(stale.cards[0]!.notice).toEqual({
      tone: "warn",
      text: "Couldn't refresh · showing data from 1h ago · included usage unavailable",
    });
  });

  it("dims stale data and explains why after a failed refresh", () => {
    const vm = buildOverlay(
      snap([p("codex", { kind: "error", message: "offline" }, usage(28, 55, 3_600_000)), ready()[1]!]),
      NOW,
    );
    const card = vm.cards[0]!;
    expect(card.dim).toBe(true);
    expect(card.notice).toMatchObject({ tone: "warn", text: "Couldn't refresh · showing data from 1h ago" });
    expect(card.rows[0]).toMatchObject({ kind: "value", remaining: 72 });
  });

  it("labels old data even without an error", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, usage(28, 55, 40 * 60_000)), ready()[1]!]), NOW);
    expect(vm.cards[0]!.notice?.text).toBe("Last updated 40m ago");
    expect(vm.updated).toBe("Updated 40m ago");
  });

  it("shows clear messages when nothing is available", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "notInstalled" }, null), p("claude", { kind: "needsConsent" }, null)]), NOW);
    expect(vm.cards[0]!.message?.title).toBe("Codex isn't installed");
    expect(vm.cards[0]!.rows).toHaveLength(0);
    expect(vm.cards[1]!.consent).toBe(true);
    expect(vm.updated).toBe("");
  });

  it("does not show cached numbers after signing out", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "signedOut", message: "Run `codex login`." }, usage(10, 10)), ready()[1]!]), NOW);
    expect(vm.cards[0]!.rows).toHaveLength(0);
    expect(vm.cards[0]!.message?.title).toBe("Not signed in");
  });

  it("uses loading placeholders on first load", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "unknown" }, null), p("claude", { kind: "unknown" }, null)], { showWeekly: false }), NOW);
    expect(vm.cards[0]!.rows).toEqual([{ key: "fiveHour", label: "5-hour", kind: "loading" }]);
  });

  it("explains an idle window with no reset time", () => {
    const u = usage(0, 10);
    u.fiveHour!.resetsAt = null;
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, u), ready()[1]!]), NOW);
    expect(vm.cards[0]!.rows[0]).toMatchObject({ kind: "value", remaining: 100, reset: "Starts with your next request" });
  });

  it("says when a limit is used up", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, usage(100, 10)), ready()[1]!]), NOW);
    expect(vm.cards[0]!.rows[0]).toMatchObject({ kind: "value", remaining: 0, display: "0%", reset: "Limit reached · resets in 1h" });
    expect(vm.cards[0]!.rows[1]).toMatchObject({ reset: expect.stringMatching(/^Resets /) });
  });

  it("shows a sliver as <1% instead of claiming the limit is reached", () => {
    const vm = buildOverlay(snap([p("codex", { kind: "ready" }, usage(99.6, 10)), ready()[1]!]), NOW);
    expect(vm.cards[0]!.rows[0]).toMatchObject({ kind: "value", remaining: 0, display: "<1%", reset: "Resets in 1h" });
  });

  it("falls back to the oldest shown data for the footer when nothing is current", () => {
    const vm = buildOverlay(
      snap([
        p("codex", { kind: "error", message: "offline" }, usage(28, 55, 2 * 3_600_000)),
        p("claude", { kind: "rateLimited", retryAt: NOW + 60_000 }, usage(62, 19, 9 * 60_000)),
      ]),
      NOW,
    );
    expect(vm.updated).toBe("Updated 2h ago");
  });

  describe("Antigravity", () => {
    const w = (used: number, minutes: number) => ({ usedPercent: used, resetsAt: sec(3_600_000), windowMinutes: minutes });
    const agUsage = (fetchedAgo = 10_000): Usage => ({
      fiveHour: null,
      weekly: null,
      plan: null,
      source: "antigravityApp",
      fetchedAt: NOW - fetchedAgo,
      groups: [
        { name: "Gemini", fiveHour: w(14, 300), weekly: w(45, 10080) },
        { name: "Claude and GPT", fiveHour: null, weekly: w(76, 10080) },
      ],
    });

    it("shows a row per model group and window", () => {
      const vm = buildOverlay(snap([...ready(), p("antigravity", { kind: "ready" }, agUsage())]), NOW);
      const card = vm.cards[2]!;
      expect(card.name).toBe("Antigravity");
      expect(card.rows.map((r) => r.label)).toEqual(["Gemini · 5-hour", "Gemini · Weekly", "Claude and GPT · 5-hour", "Claude and GPT · Weekly"]);
      expect(card.rows[0]).toMatchObject({ kind: "value", display: "86%" });
      expect(card.rows[2]).toMatchObject({ kind: "missing", note: "Not reported" });
      expect(new Set(card.rows.map((r) => r.key)).size).toBe(4);
    });

    it("follows the 5-hour / weekly settings", () => {
      const vm = buildOverlay(snap([p("antigravity", { kind: "ready" }, agUsage())], { showFiveHour: false }), NOW);
      expect(vm.cards[0]!.rows.map((r) => r.label)).toEqual(["Gemini · Weekly", "Claude and GPT · Weekly"]);
    });

    it("stays out of the way when it isn't installed", () => {
      const absent = p("antigravity", { kind: "notInstalled" }, null);
      expect(buildOverlay(snap([...ready(), absent]), NOW).cards.map((c) => c.id)).toEqual(["codex", "claude"]);
      const alone = buildOverlay(snap([absent], { showCodex: false, showClaude: false }), NOW);
      expect(alone.cards[0]!.message?.title).toBe("Antigravity isn't installed");
    });

    it("keeps the last numbers while the app is closed, saying so", () => {
      const vm = buildOverlay(snap([p("antigravity", { kind: "waiting" }, agUsage(5 * 60_000))]), NOW);
      expect(vm.cards[0]!.notice).toEqual({ tone: "muted", text: "Antigravity isn't open · data from 5m ago" });
      expect(vm.cards[0]!.rows[0]).toMatchObject({ kind: "value" });
      const never = buildOverlay(snap([p("antigravity", { kind: "waiting" }, null)]), NOW);
      expect(never.cards[0]!.message?.title).toBe("Antigravity isn't open");
    });

    it("asks before reading", () => {
      const vm = buildOverlay(snap([...ready(), p("antigravity", { kind: "needsConsent" }, null)]), NOW);
      expect(vm.cards[2]!.consent).toBe(true);
    });
  });

  it("tags status line data with its source", () => {
    const u = { ...usage(30, 30), plan: null, source: "claudeStatusLine" as const };
    const vm = buildOverlay(snap([ready()[0]!, p("claude", { kind: "ready" }, u)]), NOW);
    expect(vm.cards[1]!.badge).toBe("Status line");
  });
});
