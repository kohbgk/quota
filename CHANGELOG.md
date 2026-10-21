# Changelog

## [0.1.0]

The first release of Quota: your Codex and Claude Code subscription usage, one shortcut away.

**Downloads**

- `Quota_0.1.0_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.1.0_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**Highlights**

- <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Space</kbd> toggles a compact overlay on the monitor
  under your cursor. You can change the shortcut in Settings; Quota warns if another app already uses it.
- Remaining percentage and reset time for the 5-hour and weekly limits of Codex and Claude Code.
  Percentages are rounded down, and limits a provider doesn't report say "Not reported".
- Data that couldn't be refreshed is dimmed and says how old it is. Limits that reset since the
  last check say so instead of showing old numbers.
- Codex usage comes from the official `codex app-server` interface. Quota never reads your Codex
  sign-in.
- Claude Code usage is opt-in, with two sources to choose from: Claude Code's saved sign-in
  (undocumented Anthropic endpoint, sent only to `api.anthropic.com`, never stored by Quota) or
  the documented status line interface.
- Tray icon with a usage tooltip, launch at startup, light and dark themes, and refresh every
  2, 5, or 15 minutes or only when opened.
