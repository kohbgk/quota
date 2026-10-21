import { describe, expect, it } from "vitest";
import { formatAgo, formatDuration, formatReset, levelFor, planLabel, remainingPercent, shortcutKeys, shortenPath, inlineCode } from "./format";
import { recordKey } from "./keys";

const key = (code: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("remainingPercent", () => {
  it("shows what is left, never overstating it", () => {
    expect(remainingPercent(28)).toBe(72);
    expect(remainingPercent(27.4)).toBe(72);
    expect(remainingPercent(0)).toBe(100);
    expect(remainingPercent(0.4)).toBe(99);
    expect(remainingPercent(100)).toBe(0);
    expect(remainingPercent(140)).toBe(0);
    expect(remainingPercent(-5)).toBe(100);
    expect(remainingPercent(Number.NaN)).toBe(100);
    expect(remainingPercent(7.000000000000001)).toBe(93);
  });

  it("assigns levels", () => {
    expect(levelFor(72)).toBe("ok");
    expect(levelFor(30)).toBe("warn");
    expect(levelFor(10)).toBe("low");
    expect(levelFor(0)).toBe("low");
  });
});

describe("durations and times", () => {
  const now = Date.UTC(2026, 9, 21, 12, 0, 0);
  it("formats short durations", () => {
    expect(formatDuration(20_000)).toBe("<1m");
    expect(formatDuration(42 * 60_000)).toBe("42m");
    expect(formatDuration(2 * 3_600_000 + 14 * 60_000)).toBe("2h 14m");
    expect(formatDuration(3 * 3_600_000)).toBe("3h");
    expect(formatDuration(3 * 86_400_000 + 5 * 3_600_000)).toBe("3d 5h");
  });

  it("counts down within a day and names the day beyond", () => {
    expect(formatReset(now / 1000 + 2 * 3600 + 14 * 60, now)).toBe("Resets in 2h 14m");
    expect(formatReset(now / 1000 - 5, now)).toBe("Resetting now");
    expect(formatReset(now / 1000 + 3 * 86400, now)).toMatch(/^Resets \S+ \d/);
  });

  it("formats relative ages", () => {
    expect(formatAgo(now - 10_000, now)).toBe("just now");
    expect(formatAgo(now - 4 * 60_000, now)).toBe("4m ago");
    expect(formatAgo(now - 2 * 3_600_000, now)).toBe("2h ago");
    expect(formatAgo(now - 3 * 86_400_000, now)).toBe("3d ago");
    expect(formatAgo(now + 5000, now)).toBe("just now");
  });
});

describe("labels", () => {
  it("names plans", () => {
    expect(planLabel("plus")).toBe("Plus");
    expect(planLabel("PRO")).toBe("Pro");
    expect(planLabel("self_serve_business_usage_based")).toBe("Business");
    expect(planLabel("enterprise_cbp_automation")).toBe("Enterprise");
    expect(planLabel("ent26")).toBe("Enterprise");
    expect(planLabel("prolite")).toBe("Pro Lite");
    expect(planLabel("edu_plus")).toBe("Edu Plus");
    expect(planLabel(null)).toBeNull();
    expect(planLabel("unknown")).toBeNull();
  });

  it("splits shortcuts into keycaps", () => {
    expect(shortcutKeys("Ctrl+Shift+Space")).toEqual(["Ctrl", "Shift", "Space"]);
    expect(shortcutKeys("Alt+Win+ArrowUp")).toEqual(["Alt", "Win", "↑"]);
    expect(shortcutKeys("Ctrl+Backquote")).toEqual(["Ctrl", "`"]);
  });

  it("escapes text before adding code spans", () => {
    expect(inlineCode("Run `codex login` <now>")).toBe("Run <code>codex login</code> &lt;now&gt;");
  });
});

describe("shortcut recording", () => {
  it("waits while only modifiers are held", () => {
    expect(recordKey(key("ControlLeft", { ctrlKey: true }))).toEqual({ kind: "pending", modifiers: ["Ctrl"] });
  });

  it("produces backend accelerators", () => {
    expect(recordKey(key("Space", { ctrlKey: true, shiftKey: true }))).toEqual({ kind: "done", accelerator: "Ctrl+Shift+Space" });
    expect(recordKey(key("KeyQ", { altKey: true }))).toEqual({ kind: "done", accelerator: "Alt+Q" });
    expect(recordKey(key("Digit1", { metaKey: true, altKey: true }))).toEqual({ kind: "done", accelerator: "Alt+Win+1" });
    expect(recordKey(key("F9"))).toEqual({ kind: "done", accelerator: "F9" });
  });

  it("rejects shortcuts that would block typing", () => {
    expect(recordKey(key("KeyA")).kind).toBe("invalid");
    expect(recordKey(key("KeyA", { shiftKey: true })).kind).toBe("invalid");
    expect(recordKey(key("IntlRo", { ctrlKey: true })).kind).toBe("invalid");
  });
});

describe("shortenPath", () => {
  it("keeps short paths and elides the middle of long ones", () => {
    expect(shortenPath("C:\\Tools\\codex.exe")).toBe("C:\\Tools\\codex.exe");
    expect(
      shortenPath("C:\\Users\\you\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\vendor\\x86_64-pc-windows-msvc\\codex\\codex.exe"),
    ).toBe("C:\\Users\\you\\…\\codex\\codex.exe");
  });
});
