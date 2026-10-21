# Quota — handoff notes

Read this first if you are a new session picking up the project. The original
product brief is reproduced at the bottom of this file under "Original brief summary".

## What Quota is

A Windows tray utility: press **Ctrl+Shift+Space** anywhere and a compact overlay
shows how much **Codex** (ChatGPT plan) and **Claude Code** (Claude plan)
subscription usage is left, for the 5-hour and weekly windows. No API keys.

## Stack and layout

Tauri 2 (Rust core + WebView2) with a framework-free TypeScript UI built by Vite.

```
src-tauri/src/
  main.rs            entry; also handles --quota-statusline (Claude bridge) and --quota-cleanup (uninstaller)
  lib.rs             app setup: plugins, overlay events, tray, first run, startup refresh
  core.rs            state + refresh engine (policy, backoff, cache, 15s ticker) — unit tested
  model.rs           provider-neutral types, window classification, secret redaction
  settings.rs        persisted preferences (%APPDATA%\com.kohbgk.quota\settings.json)
  overlay.rs         show/hide/toggle, cursor-monitor placement (DPI aware), settings window
  shortcut.rs        global shortcut normalize/register/change/pause — unit tested
  tray.rs            tray icon, menu, tooltip with remaining usage
  commands.rs        all #[tauri::command]s used by the UI
  providers/codex.rs      spawns `codex app-server`, JSON-RPC account/rateLimits/read — unit tested with a fake server
  providers/claude.rs     opt-in Anthropic OAuth usage endpoint using ~/.claude/.credentials.json — unit tested
  providers/statusline.rs opt-in Claude Code status line bridge (documented rate_limits field) — unit tested
src/
  overlay.ts, settings.ts   the two pages (keyed DOM updates, no framework)
  lib/view.ts               pure view model: what the overlay shows in every state — unit tested
  lib/format.ts, keys.ts    formatting + shortcut recording — unit tested
  dev/mock.ts               mock backend for browser previews (?scenario=...), excluded from prod builds
.github/workflows/ci.yml      Windows: typecheck, vitest, clippy -D warnings, cargo test, tauri build, upload artifacts
.github/workflows/release.yml tag vX.Y.Z → builds and publishes setup .exe + portable .exe + SHA256SUMS
```

## Key decisions (keep these)

- **Codex**: official `codex app-server` over stdio (newline JSON-RPC): `initialize` →
  `initialized` → `account/rateLimits/read` (with `excludeResetCreditDetails`, falls back
  to no params on -32602). Windows are classified by `windowDurationMins` (300 = 5-hour,
  10080 = weekly), never by position. Quota never reads Codex credentials.
- **Claude**: no official API exists. Two sources, **both off until the user opts in**:
  1. `GET https://api.anthropic.com/api/oauth/usage` with `anthropic-beta: oauth-2025-04-20`
     and Claude Code's saved access token. `utilization` is 0–100 used, `resets_at` RFC 3339.
     Quota **never refreshes or stores** the token (Claude Code rotates refresh tokens; refreshing
     from Quota could sign Claude Code out). Expired token → "open Claude Code to renew".
     User-Agent is honest (`Quota/x.y.z`), not impersonating Claude Code. Verified live:
     a bogus token gets 401 → mapped to Expired (`cargo test -- --ignored`).
  2. Status line bridge: Quota registers `quota.exe --quota-statusline` as Claude Code's
     statusLine command, records `rate_limits` to `claude-statusline.json`. Never replaces an
     existing custom status line; uninstaller removes it.
- Never show fabricated numbers. Missing windows show "Not reported"; windows that reset
  since the last fetch show "Reset since last update"; stale data is dimmed with a reason.
- Remaining % is rounded **down** so Quota never overstates what's left.
- Refresh policy (core.rs): on open if data older than 60s (Codex) / 120s (Claude); timer
  2/5/15 min or open-only; Claude manual floor 30s; exponential backoff; Retry-After honored.

## Status

Done and verified on Linux:
- Rust: builds with zero warnings, clippy clean, **37 unit tests pass**.
- Frontend: `tsc` clean, **24 vitest tests pass**, production build OK.
- UI reviewed via headless-Chromium screenshots of ~15 overlay states + settings (dark/light).
- First commit pushed; CI run started: https://github.com/kohbgk/quota/actions

**Not yet verified** (needs Windows / CI):
- The Windows-only code paths: `creation_flags(CREATE_NO_WINDOW)` in codex.rs,
  `GetShortPathNameW` in statusline.rs, NSIS `windows/hooks.nsh`, the whole `tauri build` bundle.
  This Linux sandbox cannot compile for Windows (static.rust-lang.org is blocked), so CI is the
  Windows compiler. Read CI results via the public API (no auth needed for a public repo):
  `curl -s https://api.github.com/repos/kohbgk/quota/actions/runs?per_page=3`
  then `.../actions/runs/<id>/jobs` and `.../check-runs/<job_id>/annotations` for compiler errors.
- Real end-to-end usage with the user's actual Codex / Claude logins (only the user can do this).

## Next steps (in order)

1. **Get CI green on Windows.** Fix anything the first run reports. Push, re-check.
2. **UI polish found in the screenshot review** (small, all in `src/`):
   - Footer "Updated …" is blank when no provider is `ready` (e.g. one error + one rate-limited).
     In `buildOverlay` fall back to the oldest `fetchedAt` among cards that show data.
   - At 0% left, make the reset line read "Limit reached · resets in 36m".
   - First-run hint banner wraps keycaps awkwardly; drop the inline `<kbd>`s (the footer
     already shows the shortcut) — e.g. "Quota keeps running in your tray. Use the shortcut below anytime."
   - "Not reported" rows: the striped bar is a bit loud; lower its contrast.
   - Optionally tighten vertical rhythm a little (`.rows` gap 14→12, card padding 14→12 top).
   - Re-run screenshots after changes (see "Previewing the UI").
3. **Write the README** (brief §11): what/why, providers, features, install (Releases),
   shortcut, how auth works (copy the honest wording from Settings), settings, known
   limitations (undocumented Claude endpoint; status line bridge won't replace custom lines;
   portable exe needs WebView2; Windows SmartScreen on unsigned builds), build from source,
   release process (`git tag v0.1.0 && git push --tags`). Add a screenshot (dark overlay).
4. **Release v0.1.0** once CI is green: version lives in `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.toml`, `package.json` (keep in sync). Tag `v0.1.0` and push the tag.
5. Give the user a manual Windows test checklist (brief §10): both / one / no provider,
   offline, shortcut conflict (try Alt+Space), 5-hour-only / weekly-only, provider toggles,
   restart persistence, two monitors with different scaling, autostart, tray left-click, Quit.

## Working in this repo

```bash
npm ci                      # .npmrc sets legacy-peer-deps (npm 10 arborist bug otherwise)
npm test && npm run typecheck
npm run build               # frontend → dist/
cd src-tauri && cargo test && cargo clippy --all-targets
```
On Linux, compiling the Rust crate needs `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev
librsvg2-dev libxdo-dev` (apt). Windows bundles come from CI.

### Previewing the UI

`npm run dev`, then open `http://localhost:1420/?scenario=NAME` (overlay) or
`/settings.html?scenario=NAME`. Scenarios (src/dev/mock.ts): both, loading, codex-only,
five-hour-only, weekly-only, low, stale, none, consent, signedout, errors, statusline,
waiting, missing, shortcut-error, first-run, light, light-low. Add `&bridge=occupied` on settings.

For headless screenshots in a sandbox where Playwright can't download browsers, the npm
package `@sparticuz/chromium` + `puppeteer-core` works (Chromium ships inside the npm tarball).
Render at width 372 for the overlay, 600×680 for settings.

### Commit conventions

Small, descriptive commits on `main`. Never commit secrets or auth files (`.gitignore` covers
`.credentials.json`, `auth.json`, `.env*`).

## Original brief summary

Build a polished, minimal, lightweight Windows desktop app "Quota" (capital Q; repo `quota`).
Tagline: "Your AI usage. One shortcut away." Global shortcut (default Ctrl+Shift+Space,
customizable, persisted) toggles a compact overlay; Esc and click-outside dismiss; runs in the
tray; no taskbar clutter; multi-monitor + DPI correct; single instance; launch at startup.
Show remaining % and reset time for 5-hour and weekly limits for Codex and Claude Code using the
user's existing subscription logins (no API keys). Settings: show 5-hour / weekly (≥1 on),
show Codex / Claude (≥1 on), shortcut recorder with conflict handling, launch at startup,
hide on click-outside, theme, refresh behavior. Auto refresh with caching, stale vs current
clearly distinguished, never fabricate data, never log/expose tokens, undocumented mechanisms
need explicit opt-in. Professional README and GitHub Releases (installer + portable exe).
Out of scope: chat, predictions, analytics dashboards, accounts, cloud sync, extra providers.
