// Release builds are windowed apps: no console window flashes on launch.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use quota_lib::providers::statusline;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Claude Code status line bridge: runs for a few milliseconds, never starts the UI.
    if args.iter().any(|a| a == statusline::BRIDGE_FLAG) {
        statusline::run_bridge(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            &quota_lib::paths::statusline_snapshot_file(),
            quota_lib::paths::now_ms(),
        );
        return;
    }

    // Used by the uninstaller to remove Quota's integrations.
    if args.iter().any(|a| a == "--quota-cleanup") {
        quota_lib::cleanup();
        return;
    }

    quota_lib::run(args.iter().any(|a| a == quota_lib::BACKGROUND_FLAG));
}
