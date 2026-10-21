// Converts keyboard events into accelerators the backend understands.

const MODIFIER_CODES = new Set([
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
]);

export interface KeyLike {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type Recorded =
  | { kind: "pending"; modifiers: string[] }
  | { kind: "done"; accelerator: string }
  | { kind: "invalid"; message: string; modifiers: string[] };

function modifiersOf(e: KeyLike): string[] {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Win");
  return mods;
}

/** Maps `KeyboardEvent.code` to the backend's key names. */
export function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  const allowed = new Set([
    "Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown",
    "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Backquote", "Minus", "Equal", "BracketLeft",
    "BracketRight", "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash",
    "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide", "NumpadDecimal", "NumpadEnter",
  ]);
  return allowed.has(code) ? code : null;
}

/** Interprets a keydown while recording a shortcut. */
export function recordKey(e: KeyLike): Recorded {
  const modifiers = modifiersOf(e);
  if (MODIFIER_CODES.has(e.code)) return { kind: "pending", modifiers };
  const key = keyName(e.code);
  if (!key) return { kind: "invalid", message: "That key can't be used in a shortcut.", modifiers };
  const isFunctionKey = /^F\d+$/.test(key);
  const hasPrimary = modifiers.some((m) => m !== "Shift");
  if (!hasPrimary && !isFunctionKey) {
    return { kind: "invalid", message: "Include Ctrl, Alt or Win so the shortcut doesn't interfere with typing.", modifiers };
  }
  return { kind: "done", accelerator: [...modifiers, key].join("+") };
}
