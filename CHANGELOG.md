# Changelog

## [0.1.1]

Accuracy fixes, found by checking Quota against the real Codex app-server and Anthropic's
current status line documentation.

**Downloads**

- `Quota_0.1.1_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.1.1_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**Fixes**

- The tray tooltip and the Claude Code status line rounded the remaining percentage to the
  nearest whole number, so they could show 1% more than you have. They now round down, like
  the overlay.
- When a Claude Code limit resets, the status line bridge no longer reports that limit as
  "Not reported". It now shows "Reset since last update" until fresh numbers arrive.
- Any limit past its reset time now shows as reset, never with its old numbers.
- When Codex reports that included usage isn't currently allowed for your account, Quota says
  so instead of letting the percentages suggest otherwise.
- Clearer plan badges for Codex's newer plan types (for example "Business" and "Enterprise"
  instead of raw IDs).
- The tray tooltip shows the weekly limit when a plan has no 5-hour limit, and says when a limit
  has reset.

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
