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
  shortcut: "Ctrl+Shift+Space",
  hideOnBlur: true,
  theme: "system",
  refreshInterval: "fiveMinutes",
  claudeUsageEndpoint: true,
  claudeStatusLine: false,
  codexPath: null,
  onboarded: true,
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
  return { settings: { ...settings, ...overrides }, providers, shortcutError: null, launchAtStartup: true, version: "0", now: NOW, firstRun: false };
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

  it("tags status line data with its source", () => {
    const u = { ...usage(30, 30), plan: null, source: "claudeStatusLine" as const };
    const vm = buildOverlay(snap([ready()[0]!, p("claude", { kind: "ready" }, u)]), NOW);
    expect(vm.cards[1]!.badge).toBe("Status line");
  });
});
