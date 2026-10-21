//! Codex subscription usage through the official `codex app-server` JSON-RPC
//! interface (`account/rateLimits/read`).
//!
//! Codex owns its sign-in (including token refresh). Quota only starts the
//! locally installed CLI, asks for rate limits over stdio and exits it again.
//! Quota never reads Codex's credential files.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::{find_on_path, ProviderError};
use crate::model::{classify_window_minutes, redact, LimitWindow, Usage, UsageSource, WindowKind};

const SESSION_TIMEOUT: Duration = Duration::from_secs(25);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Finds the Codex CLI. An explicit path from Settings wins; otherwise common
/// install locations are searched, preferring the native executable over the
/// npm `.cmd` shim (which would start Node.js first).
pub fn locate(explicit: Option<&str>) -> Result<PathBuf, ProviderError> {
    if let Some(path) = explicit.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(ProviderError::Other(format!(
                "The Codex path set in Settings doesn't exist: {}",
                path.display()
            )))
        };
    }
    candidates().into_iter().find(|p| p.is_file()).ok_or(ProviderError::NotInstalled)
}

#[cfg(windows)]
fn candidates() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let appdata = env("APPDATA");
    let local = env("LOCALAPPDATA");
    let home = crate::paths::home_dir();
    let mut out = Vec::new();

    if let Some(p) = find_on_path(&["codex.exe"]) {
        out.push(p);
    }
    // Native binary shipped inside the npm package.
    if let Some(appdata) = &appdata {
        out.extend(find_vendored_exe(&appdata.join("npm").join("node_modules").join("@openai")));
    }
    if let Some(p) = find_on_path(&["codex.cmd", "codex.bat"]) {
        // A shim from another npm prefix: look for its vendored binary first.
        if let Some(prefix) = p.parent() {
            out.extend(find_vendored_exe(&prefix.join("node_modules").join("@openai")));
        }
        out.push(p);
    }
    if let Some(appdata) = &appdata {
        out.push(appdata.join("npm").join("codex.cmd"));
    }
    if let Some(local) = &local {
        out.push(local.join("Microsoft").join("WinGet").join("Links").join("codex.exe"));
        out.push(local.join("Programs").join("codex").join("codex.exe"));
        out.push(local.join("Volta").join("bin").join("codex.exe"));
        out.push(local.join("pnpm").join("codex.cmd"));
    }
    if let Some(home) = &home {
        out.push(home.join("scoop").join("shims").join("codex.exe"));
        out.push(home.join(".bun").join("bin").join("codex.exe"));
        out.push(home.join(".local").join("bin").join("codex.exe"));
    }
    out
}

#[cfg(not(windows))]
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = find_on_path(&["codex"]) {
        out.push(p);
    }
    if let Some(home) = crate::paths::home_dir() {
        out.push(home.join(".local/bin/codex"));
        out.push(home.join(".npm-global/bin/codex"));
    }
    out.push(PathBuf::from("/usr/local/bin/codex"));
    out.push(PathBuf::from("/opt/homebrew/bin/codex"));
    out
}

/// Finds `codex.exe` below an `@openai` npm scope directory, preferring the
/// binary built for the current CPU architecture.
#[cfg_attr(not(windows), allow(dead_code))]
fn find_vendored_exe(scope_dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth == 0 || found.len() >= 8 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                walk(&path, depth - 1, found);
            } else if entry.file_name().eq_ignore_ascii_case("codex.exe") {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(scope_dir, 8, &mut found);
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    found.sort_by_key(|p| !p.to_string_lossy().contains(arch));
    found
}

/// Starts `codex app-server`, reads rate limits and shuts it down again.
pub async fn fetch(explicit_path: Option<String>) -> Result<Usage, ProviderError> {
    let program = locate(explicit_path.as_deref())?;

    let mut command = tokio::process::Command::new(&program);
    command
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => ProviderError::NotInstalled,
        _ => ProviderError::Other(format!("Couldn't start Codex: {}", redact(&err.to_string()))),
    })?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stderr_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let _ = (&mut stderr).take(16 * 1024).read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut reader = BufReader::new(stdout);
    let outcome = tokio::time::timeout(SESSION_TIMEOUT, rpc_session(&mut reader, &mut stdin)).await;

    // Closing stdin asks app-server to exit; kill it if it lingers.
    drop(stdin);
    drop(reader);
    if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
        let _ = child.kill().await;
    }

    match outcome {
        Ok(Ok(result)) => parse_rate_limits(&result, crate::paths::now_ms()),
        Ok(Err(SessionError::Provider(err))) => Err(err),
        Ok(Err(SessionError::Closed)) => {
            let stderr = stderr_task.await.unwrap_or_default();
            Err(classify_failure_text(&stderr).unwrap_or_else(|| {
                let tail = last_meaningful_line(&stderr);
                ProviderError::Other(if tail.is_empty() {
                    "Codex exited before reporting usage.".into()
                } else {
                    format!("Codex exited before reporting usage: {}", redact(&tail))
                })
            }))
        }
        Err(_) => Err(ProviderError::Other("Codex took too long to respond.".into())),
    }
}

#[derive(Debug)]
enum SessionError {
    Provider(ProviderError),
    Closed,
}

impl From<std::io::Error> for SessionError {
    fn from(_: std::io::Error) -> Self {
        SessionError::Closed
    }
}

/// Runs the JSON-RPC conversation over newline-delimited JSON:
/// `initialize` → `initialized` → `account/rateLimits/read`.
async fn rpc_session<R, W>(reader: &mut R, writer: &mut W) -> Result<Value, SessionError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    send(
        writer,
        &json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "quota", "title": "Quota", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": null
            }
        }),
    )
    .await?;
    let init = read_response(reader, 1).await?;
    if let Some(error) = init.get("error") {
        return Err(SessionError::Provider(map_rpc_error(error)));
    }
    send(writer, &json!({ "method": "initialized" })).await?;

    // Ask for the lightweight variant (skips an extra detail lookup); fall back
    // to a bare request for older app-server versions that reject the params.
    send(
        writer,
        &json!({ "id": 2, "method": "account/rateLimits/read", "params": { "excludeResetCreditDetails": true } }),
    )
    .await?;
    let mut response = read_response(reader, 2).await?;
    if response.get("error").and_then(|e| e.get("code")).and_then(Value::as_i64) == Some(-32602) {
        send(writer, &json!({ "id": 3, "method": "account/rateLimits/read" })).await?;
        response = read_response(reader, 3).await?;
    }
    if let Some(error) = response.get("error") {
        return Err(SessionError::Provider(map_rpc_error(error)));
    }
    response.get("result").cloned().ok_or(SessionError::Provider(ProviderError::Other(
        "Codex returned an empty response.".into(),
    )))
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: &Value) -> Result<(), SessionError> {
    let mut line = serde_json::to_vec(message).expect("serialize");
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads lines until the response with `id` arrives. Notifications, server
/// requests and any non-JSON output are skipped.
async fn read_response<R: AsyncBufRead + Unpin>(reader: &mut R, id: i64) -> Result<Value, SessionError> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Err(SessionError::Closed);
        }
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let is_response = message.get("result").is_some() || message.get("error").is_some();
        if is_response && message.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(message);
        }
    }
}

fn map_rpc_error(error: &Value) -> ProviderError {
    let code = error.get("code").and_then(Value::as_i64);
    let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
    if code == Some(-32601) {
        return ProviderError::Unsupported(
            "This Codex version can't report usage. Update Codex and try again.".into(),
        );
    }
    classify_failure_text(message)
        .unwrap_or_else(|| ProviderError::Other(format!("Codex: {}", redact(message))))
}

fn classify_failure_text(text: &str) -> Option<ProviderError> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("chatgpt authentication required") {
        return Some(ProviderError::Unsupported(
            "Codex is signed in with an API key. Subscription limits are only available when signed in with ChatGPT."
                .into(),
        ));
    }
    if lower.contains("authentication required") || lower.contains("not logged in") || lower.contains("please log in") {
        return Some(ProviderError::SignedOut("Run `codex login` to sign in with ChatGPT.".into()));
    }
    if lower.contains("error sending request")
        || lower.contains("dns error")
        || lower.contains("connection refused")
        || lower.contains("timed out")
    {
        return Some(ProviderError::Network("Codex couldn't reach ChatGPT. Check your connection.".into()));
    }
    None
}

fn last_meaningful_line(text: &str) -> String {
    text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default().to_string()
}

fn get<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake)).filter(|v| !v.is_null())
}

/// Maps an `account/rateLimits/read` result to Quota's model. Windows are
/// classified by their reported duration, not by their position.
pub fn parse_rate_limits(result: &Value, now_ms: i64) -> Result<Usage, ProviderError> {
    let snapshot = get(result, "rateLimitsByLimitId", "rate_limits_by_limit_id")
        .and_then(|by_id| by_id.get("codex"))
        .filter(|v| v.is_object())
        .or_else(|| get(result, "rateLimits", "rate_limits"))
        .ok_or_else(|| ProviderError::Other("Codex returned an unexpected usage format.".into()))?;

    let mut usage = Usage {
        five_hour: None,
        weekly: None,
        plan: get(snapshot, "planType", "plan_type").and_then(Value::as_str).map(str::to_string),
        source: UsageSource::CodexAppServer,
        fetched_at: now_ms,
    };

    for (slot, fallback) in [("primary", WindowKind::FiveHour), ("secondary", WindowKind::Weekly)] {
        let Some(window) = snapshot.get(slot).filter(|w| w.is_object()) else { continue };
        let Some(used) = get(window, "usedPercent", "used_percent").and_then(Value::as_f64) else { continue };
        let minutes = get(window, "windowDurationMins", "window_minutes").and_then(Value::as_i64);
        let resets_at = get(window, "resetsAt", "resets_at").and_then(Value::as_i64);
        let kind = minutes.map(classify_window_minutes).unwrap_or(fallback);
        let parsed = LimitWindow::new(used, resets_at, minutes);
        match kind {
            WindowKind::FiveHour if usage.five_hour.is_none() => usage.five_hour = Some(parsed),
            WindowKind::Weekly if usage.weekly.is_none() => usage.weekly = Some(parsed),
            _ => {}
        }
    }
    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt};

    fn sample() -> Value {
        json!({
            "ordinaryUsageAllowed": true,
            "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 28, "windowDurationMins": 300, "resetsAt": 1_800_000_000 },
                "secondary": { "usedPercent": 55, "windowDurationMins": 10080, "resetsAt": 1_800_400_000 },
                "planType": "plus"
            },
            "rateLimitsByLimitId": null
        })
    }

    #[test]
    fn parses_primary_and_secondary_by_duration() {
        let usage = parse_rate_limits(&sample(), 42).unwrap();
        assert_eq!(usage.five_hour.as_ref().unwrap().used_percent, 28.0);
        assert_eq!(usage.five_hour.as_ref().unwrap().resets_at, Some(1_800_000_000));
        assert_eq!(usage.weekly.as_ref().unwrap().used_percent, 55.0);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.fetched_at, 42);
        assert_eq!(usage.source, UsageSource::CodexAppServer);
    }

    #[test]
    fn does_not_rely_on_window_order() {
        let mut value = sample();
        value["rateLimits"]["primary"]["windowDurationMins"] = json!(10080);
        value["rateLimits"]["secondary"]["windowDurationMins"] = json!(300);
        let usage = parse_rate_limits(&value, 0).unwrap();
        assert_eq!(usage.weekly.unwrap().used_percent, 28.0);
        assert_eq!(usage.five_hour.unwrap().used_percent, 55.0);
    }

    #[test]
    fn missing_windows_stay_missing() {
        let value = json!({ "rateLimits": {
            "primary": { "usedPercent": 10.5, "windowDurationMins": 10080, "resetsAt": null },
            "secondary": null
        }});
        let usage = parse_rate_limits(&value, 0).unwrap();
        assert!(usage.five_hour.is_none(), "a weekly-only plan must not invent a 5-hour limit");
        let weekly = usage.weekly.unwrap();
        assert_eq!(weekly.used_percent, 10.5);
        assert_eq!(weekly.resets_at, None);
    }

    #[test]
    fn prefers_codex_bucket_and_accepts_snake_case() {
        let value = json!({
            "rate_limits": { "primary": { "used_percent": 90, "window_minutes": 300 } },
            "rate_limits_by_limit_id": {
                "codex": { "primary": { "used_percent": 12, "window_minutes": 300 } }
            }
        });
        assert_eq!(parse_rate_limits(&value, 0).unwrap().five_hour.unwrap().used_percent, 12.0);
    }

    #[test]
    fn maps_auth_errors() {
        let signed_out = map_rpc_error(&json!({"code": -32600, "message": "codex account authentication required to read rate limits"}));
        assert!(matches!(signed_out, ProviderError::SignedOut(_)));
        let api_key = map_rpc_error(&json!({"code": -32600, "message": "chatgpt authentication required to read rate limits"}));
        assert!(matches!(api_key, ProviderError::Unsupported(_)));
        let old = map_rpc_error(&json!({"code": -32601, "message": "method not found"}));
        assert!(matches!(old, ProviderError::Unsupported(_)));
    }

    /// Drives `rpc_session` against a scripted fake app-server.
    async fn run_fake(server_fn: impl Fn(&[Value]) -> Vec<Value> + Send + 'static) -> Result<Value, SessionError> {
        let (client_side, server_side) = duplex(64 * 1024);
        let (client_read, mut client_write) = tokio::io::split(client_side);
        let (mut server_read, mut server_write) = tokio::io::split(server_side);
        let server_task = tokio::spawn(async move {
            // Read requests as they arrive and answer from the script.
            let mut received = Vec::new();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = server_read.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=pos).collect();
                    received.push(serde_json::from_slice::<Value>(&line).unwrap());
                    for reply in server_fn(&received) {
                        let mut bytes = serde_json::to_vec(&reply).unwrap();
                        bytes.push(b'\n');
                        server_write.write_all(&bytes).await.unwrap();
                    }
                }
            }
        });
        let mut reader = BufReader::new(client_read);
        let result = rpc_session(&mut reader, &mut client_write).await;
        server_task.abort();
        result
    }

    fn script_reply(received: &[Value], rate_limits: Value) -> Vec<Value> {
        let last = received.last().unwrap();
        match last["method"].as_str() {
            Some("initialize") => vec![
                json!("not json at all"),
                json!({"method": "some/notification", "params": {}}),
                json!({"id": 1, "result": {"userAgent": "codex"}}),
            ],
            Some("account/rateLimits/read") => {
                let id = last["id"].clone();
                vec![json!({"method": "account/rateLimits/updated", "params": {}}), json!({"id": id, "result": rate_limits})]
            }
            _ => vec![],
        }
    }

    #[tokio::test]
    async fn speaks_the_app_server_protocol() {
        let result = run_fake(|received| {
            if received.last().unwrap()["method"] == "account/rateLimits/read" {
                assert_eq!(received[1]["method"], "initialized", "must ack initialize first");
                assert!(received[1].get("id").is_none(), "initialized is a notification");
                assert_eq!(received[0]["params"]["clientInfo"]["name"], "quota");
            }
            script_reply(received, sample())
        })
        .await
        .unwrap();
        assert_eq!(parse_rate_limits(&result, 0).unwrap().weekly.unwrap().used_percent, 55.0);
    }

    #[tokio::test]
    async fn retries_without_params_for_older_servers() {
        let result = run_fake(|received| {
            let last = received.last().unwrap();
            if last["id"] == 2 {
                return vec![json!({"id": 2, "error": {"code": -32602, "message": "invalid params"}})];
            }
            if last["id"] == 3 {
                assert!(last.get("params").is_none());
            }
            script_reply(received, sample())
        })
        .await
        .unwrap();
        assert!(parse_rate_limits(&result, 0).unwrap().five_hour.is_some());
    }

    #[tokio::test]
    async fn surfaces_sign_in_errors() {
        let result = run_fake(|received| {
            let last = received.last().unwrap();
            if last["id"] == 2 {
                return vec![json!({"id": 2, "error": {"code": -32600, "message": "codex account authentication required to read rate limits"}})];
            }
            script_reply(received, sample())
        })
        .await;
        assert!(matches!(result, Err(SessionError::Provider(ProviderError::SignedOut(_)))));
    }

    #[test]
    fn explicit_path_must_exist() {
        assert!(matches!(locate(Some("/definitely/not/here/codex")), Err(ProviderError::Other(_))));
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("codex");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(locate(Some(exe.to_str().unwrap())).unwrap(), exe);
    }

    #[test]
    fn finds_vendored_binary_for_current_arch() {
        let dir = tempfile::tempdir().unwrap();
        let other = if cfg!(target_arch = "aarch64") { "x86_64" } else { "aarch64" };
        let mine = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
        for arch in [other, mine] {
            let p = dir.path().join("codex/vendor").join(format!("{arch}-pc-windows-msvc/codex"));
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("codex.exe"), "").unwrap();
        }
        let found = find_vendored_exe(dir.path());
        assert_eq!(found.len(), 2);
        assert!(found[0].to_string_lossy().contains(mine));
    }
}
