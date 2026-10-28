# Manual test checklist (Windows)

Run before each release on a real Windows 10/11 PC. CI covers typecheck, unit tests,
clippy and the build. These cover what only a real desktop can.

**Install and first run**
- [ ] Installer runs without admin. SmartScreen: *More info* → *Run anyway* works.
- [ ] Quota appears in the tray, not the taskbar. The first overlay shows the "keeps running in your tray" hint; *Got it* dismisses it.
- [ ] Portable exe runs too. Launching either a second time doesn't start a second copy.

**Providers**
- [ ] Both signed in: both cards show 5-hour and weekly with plausible numbers. Compare with
      `codex` `/status` and Claude Code `/usage`.
- [ ] Claude card asks for consent first; *Allow* loads usage; *Other options* opens Settings → Providers.
- [ ] Codex not installed (or rename it temporarily): "Codex isn't installed" + *Set path…* works.
- [ ] `codex logout`: "Not signed in"; no old numbers shown.
- [ ] Status line bridge: turn on, send a Claude Code message, data appears with a "Status line" badge.
      With a custom `statusLine` already in `~\.claude\settings.json`, Quota refuses to replace it.
- [ ] Only one provider installed: the other card explains why, and the overlay still works.
- [ ] Antigravity installed and open: the card asks first; *Allow* shows Gemini and Claude/GPT 5-hour and
      weekly rows matching Antigravity's own Settings → quota page. Close Antigravity: the card says it
      isn't open and keeps the last numbers. Reopen it and refresh (R): fresh numbers.
- [ ] Antigravity not installed: no Antigravity card (unless it's the only provider shown).

**Overlay and shortcut**
- [ ] Ctrl+Shift+Space toggles from any app; Esc and click-outside hide it; R refreshes; Ctrl+, opens Settings.
- [ ] Change the shortcut (e.g. Ctrl+Alt+Q): works immediately and after restart.
- [ ] Try a taken one (e.g. Alt+Space or one another app holds): clear error and the old shortcut keeps working.
- [ ] Two monitors with different scaling (100% / 150%): overlay opens on the cursor's monitor, crisp and fully on screen.
- [ ] Drag the overlay by any empty area (not a button): it moves; plain clicks on text don't move it.
      Hide and reopen, then restart Quota: it comes back to the same spot. Drag it to the other monitor:
      it reopens there. Settings → General → Position → Reset puts it back in the default spot.
- [ ] Click outside the overlay right after Quota starts (before clicking the overlay itself): it hides.
- [ ] A small or high-scaling screen (e.g. 1366×768 at 150%): the overlay and Settings fit on screen and scroll.
- [ ] While recording a new shortcut, close Settings: the old shortcut still works.
- [ ] Toggle several settings quickly: none flips back after a moment.
- [ ] The overlay's settings button and the tray's Settings… both open Settings every time; a double-click opens one window.
- [ ] Tray left-click toggles; right-click menu: Show Quota, Refresh now, Settings…, Quit Quota. Tooltip shows usage left.

**Settings**
- [ ] 5-hour only / weekly only: overlay rows follow; the last remaining toggle can't be turned off.
- [ ] Codex only / Claude only: same.
- [ ] Theme System / Light / Dark applies to both windows.
- [ ] Refresh interval and "Only when opened" behave; "Hide when clicking outside" off keeps it open.
- [ ] Restart Quota: every setting is kept.

**Failure modes**
- [ ] Wi-Fi off: cards dim with "Couldn't refresh · showing data from …"; recovering clears it.
- [ ] Leave it a few hours: a window that reset shows "Reset since last update" until refreshed.

**Updates**
- [ ] With an older version installed: within a minute of starting, a Windows notification says a new
      version is available (once per version), the overlay shows the bar, and the tray menu says
      "Download Quota x.y.z…". **Download** opens the release page; **Later** hides the bar for that version.
- [ ] Settings → General → Updates shows the version and "checked … ago"; **Check now** works; turning it
      off stops the bar and the automatic checks.

**Startup and uninstall**
- [ ] Launch at startup on: Quota starts silently after sign-in. Off: it doesn't.
- [ ] Quit Quota from the tray exits fully (no process left).
- [ ] Uninstall removes the startup entry and Quota's status line bridge (if enabled).
