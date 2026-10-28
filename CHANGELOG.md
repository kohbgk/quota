# Changelog

## [0.2.2]

Antigravity is now off by default.

**Downloads**

- `Quota_0.2.2_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.2.2_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**Changes**

- The overlay shows Codex and Claude Code by default. Antigravity is hidden and off until you
  turn it on in Settings → Overlay → Antigravity; Quota then asks before reading its quota.
- If you upgraded from 0.2.0 or 0.2.1 without connecting Antigravity, it's hidden now. If you did
  connect it, nothing changes.

## [0.2.1]

Quota now tells you when there's a new version.

**Downloads**

- `Quota_0.2.1_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.2.1_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**New**

- **Update notices.** Quota checks for a new release shortly after it starts and every few hours.
  When there is one, you get a Windows notification and an "Update available" bar in the overlay.
  **Download** opens the release page to install it; **Later** hides the bar until the next version.
  The tray menu also offers the download.
- Settings → General → **Updates** shows your version, has **Check now**, and can turn the checks off.
- The check asks GitHub's public releases API for this repository only. No account, and nothing
  about you or your usage is sent.

This is the last version you need to find by yourself: from here on, Quota tells you.

## [0.2.0]

Antigravity support.

**Downloads**

- `Quota_0.2.0_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.2.0_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**New**

- **Antigravity**: Quota shows the 5-hour and weekly limits for each Antigravity model group
  (Gemini; Claude and GPT), read from the Antigravity app while it's open.
  - Off until you allow it: Google doesn't offer an official way to read these quotas, so Quota
    uses the app's undocumented local interface. Nothing leaves your PC.
  - When Antigravity is closed, its card keeps the last numbers and says how old they are.
  - If Antigravity isn't installed, its card stays out of the way.
- Settings: show or hide Antigravity in the overlay, and allow or stop reading its quota.

## [0.1.4]

More reliability fixes, after checking a list of suggested changes against the code.

**Downloads**

- `Quota_0.1.4_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.1.4_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**Fixes**

- When Anthropic asks Quota to slow down, Quota now waits as long as asked, even while it shows
  status line data in the meantime. It also understands date-style `Retry-After` replies and
  waits longer than an hour when told to.
- Status line data never replaces newer numbers, and when it's old the reason the sign-in
  source failed stays visible.
- Changing the Codex path or turning a Claude source off while a refresh is running no longer
  lets the old result through.
- Changing several settings quickly no longer flips earlier changes back, and "Codex path saved"
  appears only when it was.
- A sliver of usage left shows as "<1% left" instead of "Limit reached".
- Error messages and "try again in…" countdowns stay up to date while the overlay is open.
- Closing Settings while recording a shortcut no longer leaves the shortcut turned off; if
  saving a new shortcut or the status line bridge fails, the previous state is restored.
- On small or high-scaling screens, the overlay and Settings fit on screen and scroll.
- If the portable exe moves, Claude Code's status line follows it.
- Settings switches have proper screen-reader labels.

## [0.1.3]

Reliability fixes from a full code review.

**Downloads**

- `Quota_0.1.3_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.1.3_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**Fixes**

- Saving the overlay's position at the same moment as another setting could lose one of the
  changes. Settings are now always saved one change at a time.
- The overlay's saved spot is only ever where you dragged it. Quota keeping a growing overlay
  on screen, or Windows moving it during a display change, no longer overwrites it.
- Pressing and holding the tray icon while the overlay is open no longer reopens it.
- "Included usage unavailable" is still shown when the card is also showing old data.
- The tray tooltip stops showing old numbers once a limit resets, even when refresh is set to
  "Only when opened".

## [0.1.2]

Settings opens reliably, the overlay can be moved anywhere, and clicking away always hides it.

**Downloads**

- `Quota_0.1.2_x64-setup.exe`: installer for Windows 10/11 (per-user, no admin needed). Recommended.
- `Quota_0.1.2_x64-portable.exe`: a single executable. Needs WebView2, which Windows 11 and most
  Windows 10 PCs already have.
- `SHA256SUMS.txt`: checksums for both.

The builds aren't code-signed yet, so Windows SmartScreen may warn on first run: choose
*More info* → *Run anyway*.

**New**

- Drag the overlay anywhere by any empty area. It reopens in the same spot, on the same
  monitor, even after a restart. Settings → General → Position → *Reset* puts it back.

**Fixes**

- Opening Settings could freeze Quota on Windows: the window was created on the main thread,
  which WebView2 doesn't allow. Settings now opens from the overlay and the tray every time.
- The Settings window and the overlay could stay invisible if their page loaded while hidden.
  Both now always appear.
- Clicking outside the overlay now hides it even when Windows hadn't given it focus (for
  example right after Quota starts).

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
