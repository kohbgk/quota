# Quota

Windows tray app (Tauri 2 + Rust + vanilla TypeScript) that shows Codex, Claude Code and
Antigravity subscription usage in an overlay toggled by a global shortcut.

**Start by reading `HANDOFF.md`**: it has the architecture, the decisions that must be kept,
what's verified, and the ordered list of next steps.

Rules:
- Never display fabricated or estimated usage as real data.
- Never log, store, or send credentials anywhere except the provider's own server.
- Undocumented integrations stay opt-in with a clear explanation.
- Keep the scope to the brief: Codex, Claude Code and Antigravity (added at the owner's request);
  no further providers, analytics, or accounts unless the owner asks.
- Before pushing: `npm test && npm run typecheck`, and in `src-tauri/`: `cargo test && cargo clippy --all-targets`.
- Windows-only code can't be compiled in a Linux sandbox; CI (`.github/workflows/ci.yml`) is the Windows check.
