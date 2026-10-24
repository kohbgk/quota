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
- Rust: builds with zero warnings, clippy clean, **51 unit tests pass** (+2 opt-in live tests, `--ignored`).
- Frontend: `tsc` clean, **30 vitest tests pass**, production build OK.
- UI reviewed via headless-Chromium screenshots of ~15 overlay states + settings (dark/light).
- Windows CI green (typecheck, vitest, clippy -D warnings, cargo test, tauri build): https://github.com/kohbgk/quota/actions

**Not yet verified** (needs a real Windows desktop):
- Windows-only code paths compile and the bundle builds in CI, but haven't been exercised:
  `CREATE_NO_WINDOW` in codex.rs, `GetShortPathNameW` in statusline.rs, NSIS `windows/hooks.nsh`.
  This Linux sandbox cannot compile for Windows (static.rust-lang.org is blocked), so CI is the
  Windows compiler. Read CI results via the public API (no auth needed for a public repo):
  `curl -s https://api.github.com/repos/kohbgk/quota/actions/runs?per_page=3`
  then `.../actions/runs/<id>/jobs` and `.../check-runs/<job_id>/annotations` for compiler errors.
- Real end-to-end usage with the user's actual Codex / Claude logins (only the user can do this).

## Next steps

Session 2 finished the original list: Windows CI green (typecheck, vitest, clippy
`-D warnings`, cargo test, `tauri build`), UI polish (limit-reached reset line, footer
fallback, calmer first-run banner and "Not reported" bar, tighter rhythm), full README with
`docs/overlay-dark.png`, settings polish (middle-shortened Codex path, "Status line" source
label), `CHANGELOG.md` as release notes, `docs/TESTING.md`, and **v0.1.0 published**:
https://github.com/kohbgk/quota/releases/tag/v0.1.0 (setup + portable exe + SHA256SUMS,
checksums verified). Cloud sessions can't push tags (the git gateway hangs up), so the
release was started with the Release workflow's `workflow_dispatch` on main, which tags
`v<version>` itself.

Later in session 2, the integrations were checked against reality and **v0.1.1** was released
(https://github.com/kohbgk/quota/releases/tag/v0.1.1, now "latest"):
- Codex: the protocol matches `codex app-server generate-ts` output from Codex 0.163, and a
  live app-server returns exactly the errors Quota maps. Signed out gives "codex account
  authentication required…" (Not signed in); an API key gives "chatgpt authentication
  required…" (Usage isn't available). Live test:
  `npm i @openai/codex` then `QUOTA_CODEX_BIN=<vendor codex binary> cargo test -- --ignored`.
- Claude status line: the field names match https://code.claude.com/docs/en/statusline. Claude
  Code drops a window once it resets, so the bridge keeps it and the UI shows it as reset.
- Fixed: the tray and status line rounded remaining % to nearest (overstated by up to 1%); all
  surfaces now use the round-down `remaining_percent()`.
- Codex `ordinaryUsageAllowed: false` is surfaced as "Included usage isn't available right now".

**v0.1.2** (user reported: Settings did nothing and the app froze; wanted a movable overlay
that remembers its spot; click away must hide):
- Root cause: `WebviewWindowBuilder::build()` on the main thread (sync command + tray menu
  handler) deadlocks WebView2. Settings is now built on a worker thread (`overlay::open_settings`).
- Hidden windows get no `requestAnimationFrame`; the pages' ready/show signals now use
  `afterRender` (src/lib/frame.ts) with a timer fallback, plus a Rust fallback show for Settings.
- Drag: JS starts `start_overlay_drag` after 4px of movement. The spot is saved as
  `Settings.overlay_position` (work-area fractions + monitor name) once moves settle, and on
  hide/quit. Settings > General > Position > Reset.
- Click away: on Windows a watcher thread (GetAsyncKeyState) hides on a mouse press outside the
  overlay while it's open, since focus events miss clicks when the overlay never got focus.
- The real app runs headless for testing: `apt install xvfb openbox xdotool dbus-x11`, run
  `npx vite` and `Xvfb :99` + `openbox`, then start `target/debug/quota` under
  `dbus-run-session` with `WEBKIT_DISABLE_COMPOSITING_MODE=1` and a throwaway
  `HOME`/`XDG_CONFIG_HOME`. Drive it with xdotool. Linux-only quirk: the global shortcut doesn't
  hide the overlay while it has keyboard focus under X11 (Windows uses RegisterHotKey).

**v0.1.3** (full code review): all settings writes go through `Core::modify_settings` (read,
change and save under one lock; temp files unique per write). `on_moved` ignores Quota's own
placements/nudges (`placed_at`) and moves while hidden. The click-away watcher dates the hide
from the button release (tray long press). Blocked usage is noted alongside other notices.
The ticker updates the tray when a limit resets with refresh set to on-open. Release workflow
reads ref names from env vars.

**v0.1.4** (verified items from a user-supplied list of 33 suggestions): provider
`generation` drops results from before a config change; Claude's status line fallback lives in
`core::apply_result` and keeps the endpoint's backoff/rate limit (`Tracked.rate_limited`);
Retry-After HTTP dates; settings page save queue + `adopt`; `<1%`; message text refresh;
shortcut re-registered when Settings is destroyed; bridge `repair` at startup; overlay height
capped to the work area (page scrolls); switch aria-labels. Declined or deferred, with reasons
given to the user: auto-update (needs a signing secret only the owner can add), per-provider
freshness labels, cooldown hints, diagnostics, a file picker.

Remaining:

1. **Manual Windows testing by the user** with `docs/TESTING.md`. Fix whatever it turns up;
   for a fix release bump the version in all three files, add a CHANGELOG section, and
   run the Release workflow on main.
2. Code signing would remove the SmartScreen warning (needs a certificate from the user).

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
