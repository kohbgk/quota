//! Antigravity usage, read from the running Antigravity app (opt-in).
//!
//! Google doesn't document an API for Antigravity quotas. While the app runs, its
//! language server answers Connect-RPC JSON on 127.0.0.1, guarded by a CSRF token
//! that the app passes on the server's command line. `RetrieveUserQuotaSummary`
//! returns quota groups ("Gemini Models", "Claude and GPT models"), each with a
//! 5-hour and a weekly bucket. The token is read from the process when needed,
//! kept in memory only, sent only to that local server, and never logged.
//!
//! Response shapes follow what open-source readers have captured (CodexBar,
//! codeburn, ai-usage-tracker); buckets that can't be identified as a 5-hour or
//! weekly limit, or that report no remaining fraction, are left out rather than
//! guessed.

use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use super::{ProviderError, Secret};
use crate::model::{LimitWindow, QuotaGroup, Usage, UsageSource, WindowKind};

const SUMMARY_PATH: &str = "/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// A running language server that belongs to Antigravity.
#[derive(Debug)]
struct Candidate {
    pid: u32,
    token: Secret,
    /// Plain-HTTP extension server some versions answer on.
    extension_port: Option<u16>,
    /// The editor variant, whose quota payload can lack the summary groups.
    ide: bool,
}

/// The server that answered last time, tried first on the next refresh.
struct Endpoint {
    port: u16,
    https: bool,
    token: Secret,
}

static LAST_ENDPOINT: Mutex<Option<Endpoint>> = Mutex::new(None);

pub async fn fetch(enabled: bool) -> Result<Usage, ProviderError> {
    tauri::async_runtime::spawn_blocking(move || read(enabled))
        .await
        .map_err(|_| ProviderError::Other("Reading Antigravity usage was interrupted.".into()))?
}

fn read(enabled: bool) -> Result<Usage, ProviderError> {
    if !enabled {
        let present = is_installed() || !find_candidates().is_empty();
        return Err(if present { ProviderError::NeedsConsent } else { ProviderError::NotInstalled });
    }
    // Taken out first: a guard in the `if let` itself would stay locked for the whole block.
    let last = LAST_ENDPOINT.lock().unwrap().take();
    if let Some(endpoint) = last {
        if let Probe::Quota(groups) = probe(endpoint.port, endpoint.https, &endpoint.token) {
            *LAST_ENDPOINT.lock().unwrap() = Some(endpoint);
            return Ok(usage(groups));
        }
    }
    let candidates = find_candidates();
    if candidates.is_empty() {
        return Err(if is_installed() { ProviderError::Waiting } else { ProviderError::NotInstalled });
    }
    let mut no_quota = false;
    for candidate in candidates {
        let mut endpoints: Vec<(u16, bool)> = listening_ports(candidate.pid).into_iter().map(|p| (p, true)).collect();
        if let Some(port) = candidate.extension_port {
            endpoints.push((port, false));
        }
        for (port, https) in endpoints {
            match probe(port, https, &candidate.token) {
                Probe::Quota(groups) => {
                    *LAST_ENDPOINT.lock().unwrap() = Some(Endpoint { port, https, token: candidate.token });
                    return Ok(usage(groups));
                }
                Probe::NoQuota => no_quota = true,
                Probe::Failed => {}
            }
        }
    }
    Err(if no_quota {
        ProviderError::Unsupported(
            "This version of Antigravity doesn't share its quota. Update Antigravity, then try again.".into(),
        )
    } else {
        ProviderError::Other("Antigravity is running, but Quota couldn't read its quota.".into())
    })
}

fn usage(groups: Vec<QuotaGroup>) -> Usage {
    Usage {
        five_hour: None,
        weekly: None,
        plan: None,
        source: UsageSource::AntigravityApp,
        fetched_at: crate::paths::now_ms(),
        blocked: false,
        groups,
    }
}

// ---------- Finding the server ----------

fn find_candidates() -> Vec<Candidate> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_exe(UpdateKind::Always),
    );
    let mut candidates: Vec<Candidate> = system
        .processes()
        .values()
        .filter_map(|process| {
            let names = [
                Some(process.name().to_string_lossy().into_owned()),
                process.exe().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()),
            ];
            let args: Vec<String> = process.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect();
            candidate(process.pid().as_u32(), &names.into_iter().flatten().collect::<Vec<_>>(), &args)
        })
        .collect();
    // The app's own server carries the full summary; the editor variant comes last.
    candidates.sort_by_key(|c| c.ide);
    candidates
}

/// Recognizes Antigravity's language server from its executable names and arguments.
fn candidate(pid: u32, names: &[String], args: &[String]) -> Option<Candidate> {
    let is_server = names
        .iter()
        .chain(args.first())
        .filter_map(|n| n.rsplit(['/', '\\']).next())
        .any(|n| n.to_ascii_lowercase().starts_with("language_server") || n.to_ascii_lowercase().starts_with("language-server"));
    if !is_server {
        return None;
    }
    // Values never contain spaces, so splitting also copes with a command line
    // reported as one string.
    let tokens: Vec<&str> = args.iter().flat_map(|a| a.split_whitespace()).collect();
    let app_dir = flag(&tokens, "--app_data_dir").map(str::to_ascii_lowercase);
    let joined = args.join(" ").to_ascii_lowercase().replace('\\', "/");
    let antigravity = matches!(app_dir.as_deref(), Some("antigravity" | "antigravity-ide")) || joined.contains("/antigravity/");
    if !antigravity {
        return None;
    }
    let token = flag(&tokens, "--csrf_token").filter(|t| !t.is_empty())?;
    Some(Candidate {
        pid,
        token: Secret(token.to_string()),
        extension_port: flag(&tokens, "--extension_server_port").and_then(|p| p.parse().ok()).filter(|p| *p > 0),
        ide: app_dir.as_deref() == Some("antigravity-ide") || joined.contains("antigravity-ide"),
    })
}

/// `--flag value` or `--flag=value`, without surrounding quotes.
fn flag<'a>(tokens: &[&'a str], name: &str) -> Option<&'a str> {
    let value = tokens.iter().enumerate().find_map(|(i, t)| {
        if *t == name {
            tokens.get(i + 1).copied()
        } else {
            t.strip_prefix(name).and_then(|rest| rest.strip_prefix('='))
        }
    })?;
    Some(value.trim_matches('"'))
}

fn is_installed() -> bool {
    install_paths().iter().any(|p| p.exists())
}

fn install_paths() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = crate::paths::home_dir();
    if cfg!(windows) {
        [env("LOCALAPPDATA").map(|d| d.join("Programs").join("Antigravity")), env("APPDATA").map(|d| d.join("Antigravity"))]
            .into_iter()
            .flatten()
            .collect()
    } else if cfg!(target_os = "macos") {
        let mut paths = vec![PathBuf::from("/Applications/Antigravity.app")];
        paths.extend(home.map(|h| h.join("Library").join("Application Support").join("Antigravity")));
        paths
    } else {
        let mut paths = vec![PathBuf::from("/usr/share/antigravity"), PathBuf::from("/opt/Antigravity")];
        paths.extend(dirs::config_dir().map(|d| d.join("Antigravity")));
        paths
    }
}

/// TCP ports the process listens on.
fn listening_ports(pid: u32) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        std::process::Command::new(root.join("System32").join("netstat.exe"))
            .args(["-ano", "-p", "TCP"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|out| parse_netstat(&String::from_utf8_lossy(&out.stdout), pid))
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("lsof")
            .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", &pid.to_string()])
            .output()
            .map(|out| parse_lsof(&String::from_utf8_lossy(&out.stdout)))
            .unwrap_or_default()
    }
}

/// `netstat -ano` rows are `Proto Local Foreign State PID`. The state word is
/// localized, so listening sockets are recognized by their unset foreign address.
#[cfg(any(windows, test))]
fn parse_netstat(output: &str, pid: u32) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in output.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let [proto, local, foreign, .., owner] = cols.as_slice() else { continue };
        if !proto.eq_ignore_ascii_case("TCP") || owner.parse() != Ok(pid) {
            continue;
        }
        if !(foreign.ends_with(":0") || *foreign == "*:*") {
            continue;
        }
        let Some((host, port)) = local.rsplit_once(':') else { continue };
        if !matches!(host, "127.0.0.1" | "0.0.0.0" | "[::1]" | "[::]") {
            continue;
        }
        if let Ok(port) = port.parse::<u16>() {
            if port > 0 && !ports.contains(&port) {
                ports.push(port);
            }
        }
    }
    ports
}

#[cfg(any(not(windows), test))]
fn parse_lsof(output: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in output.lines().filter(|l| l.contains("(LISTEN)")) {
        let Some(address) = line.split_whitespace().rev().nth(1) else { continue };
        if let Some(Ok(port)) = address.rsplit_once(':').map(|(_, p)| p.parse::<u16>()) {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
    }
    ports
}

// ---------- Asking it ----------

enum Probe {
    Quota(Vec<QuotaGroup>),
    /// Answered, but without a quota summary (older or editor-only versions).
    NoQuota,
    Failed,
}

/// For 127.0.0.1 only: the language server uses a self-signed certificate, and
/// system proxy settings must not apply to a local connection.
fn local_agent() -> ureq::Agent {
    use ureq::tls::TlsConfig;
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .proxy(None)
        .tls_config(TlsConfig::builder().disable_verification(true).build())
        .build()
        .into()
}

fn probe(port: u16, https: bool, token: &Secret) -> Probe {
    let scheme = if https { "https" } else { "http" };
    let url = format!("{scheme}://127.0.0.1:{port}{SUMMARY_PATH}");
    let response = local_agent()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("X-Codeium-Csrf-Token", token.expose())
        .send("{}");
    let Ok(response) = response else { return Probe::Failed };
    if response.status().as_u16() != 200 {
        return Probe::Failed;
    }
    let text = response.into_body().with_config().limit(1024 * 1024).read_to_string().unwrap_or_default();
    match serde_json::from_str::<Value>(&text).ok().map(|v| parse_summary(&v)) {
        Some(groups) if !groups.is_empty() => Probe::Quota(groups),
        Some(_) => Probe::NoQuota,
        None => Probe::Failed,
    }
}

// ---------- Reading the answer ----------

fn get<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake)).filter(|v| !v.is_null())
}

/// Maps a `RetrieveUserQuotaSummary` response (2.16+ wraps it in `response`).
pub fn parse_summary(value: &Value) -> Vec<QuotaGroup> {
    let root = get(value, "response", "response").or_else(|| get(value, "summary", "summary")).unwrap_or(value);
    let Some(groups) = root.get("groups").and_then(Value::as_array) else { return Vec::new() };
    groups.iter().filter_map(parse_group).collect()
}

fn parse_group(group: &Value) -> Option<QuotaGroup> {
    let raw = get(group, "displayName", "display_name").or_else(|| group.get("name")).and_then(Value::as_str);
    let mut parsed = QuotaGroup { name: group_name(raw.unwrap_or("")), five_hour: None, weekly: None };
    for bucket in group.get("buckets").and_then(Value::as_array).into_iter().flatten() {
        if bucket.get("disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(kind) = bucket_kind(bucket) else { continue };
        let Some(remaining) = remaining_fraction(bucket) else { continue };
        let minutes = if kind == WindowKind::FiveHour { 300 } else { 10_080 };
        let window = LimitWindow::new((1.0 - remaining) * 100.0, reset_time(bucket), Some(minutes));
        let slot = if kind == WindowKind::FiveHour { &mut parsed.five_hour } else { &mut parsed.weekly };
        slot.get_or_insert(window);
    }
    (parsed.five_hour.is_some() || parsed.weekly.is_some()).then_some(parsed)
}

/// "Gemini Models" → "Gemini", "Claude and GPT models" → "Claude and GPT".
fn group_name(raw: &str) -> String {
    let name = raw.trim();
    let trimmed = name.strip_suffix(" Models").or_else(|| name.strip_suffix(" models")).unwrap_or(name).trim();
    if trimmed.is_empty() { "Models".into() } else { trimmed.into() }
}

/// Identifies the window from the bucket's id, name or window field; anything else is skipped.
fn bucket_kind(bucket: &Value) -> Option<WindowKind> {
    let text = ["bucketId", "bucket_id", "displayName", "display_name", "window", "id", "name"]
        .iter()
        .filter_map(|k| bucket.get(*k).and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if text.contains("week") {
        Some(WindowKind::Weekly)
    } else if ["5h", "5-hour", "5 hour", "five", "session"].iter().any(|k| text.contains(k)) {
        Some(WindowKind::FiveHour)
    } else {
        None
    }
}

/// Remaining fraction (0–1), in any of the shapes seen: on the bucket, nested in
/// `remaining`, or as a protobuf oneof `{ case, value }`. Protobuf JSON leaves out
/// zero values, so a missing fraction is unknown, not "used up".
fn remaining_fraction(bucket: &Value) -> Option<f64> {
    let direct = get(bucket, "remainingFraction", "remaining_fraction").and_then(Value::as_f64);
    let nested = bucket.get("remaining").and_then(|r| {
        get(r, "remainingFraction", "remaining_fraction").and_then(Value::as_f64).or_else(|| {
            (r.get("case").and_then(Value::as_str) == Some("remainingFraction"))
                .then(|| r.get("value").and_then(Value::as_f64))
                .flatten()
        })
    });
    direct.or(nested).filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0))
}

/// Unix seconds from an RFC 3339 string or a number (seconds or milliseconds).
fn reset_time(bucket: &Value) -> Option<i64> {
    let value = get(bucket, "resetTime", "reset_time").or_else(|| bucket.get("remaining").and_then(|r| get(r, "resetTime", "reset_time")))?;
    if let Some(text) = value.as_str() {
        return chrono::DateTime::parse_from_rfc3339(text).ok().map(|d| d.timestamp());
    }
    value.as_f64().map(|n| if n > 1e12 { (n / 1000.0) as i64 } else { n as i64 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Shape captured from Antigravity 2.16 (numbers made up), as used in CodexBar's tests.
    fn summary() -> Value {
        json!({ "response": {
            "description": "Within each group, models share a weekly limit and a 5-hour limit.",
            "groups": [
                { "displayName": "Gemini Models", "buckets": [
                    { "bucketId": "gemini-weekly", "displayName": "Weekly Limit",
                      "remaining": { "remainingFraction": 0.82 }, "resetTime": "2026-06-19T08:45:39Z" },
                    { "bucketId": "gemini-5h", "displayName": "Five Hour Limit",
                      "remaining": { "remainingFraction": 0.91 }, "resetTime": "2026-06-15T11:39:34Z" }
                ]},
                { "displayName": "Claude and GPT models", "buckets": [
                    { "bucketId": "3p-weekly", "displayName": "Weekly Limit",
                      "remainingFraction": 0.64, "resetTime": "2026-06-20T00:39:54Z" },
                    { "bucketId": "3p-5h", "displayName": "Five Hour Limit",
                      "remaining": { "case": "remainingFraction", "value": 0.73 }, "resetTime": "2026-06-15T12:52:10Z" }
                ]}
            ]
        }})
    }

    fn used(w: &Option<LimitWindow>) -> i64 {
        w.as_ref().map(|w| w.used_percent.round() as i64).unwrap()
    }

    #[test]
    fn reads_both_groups_and_both_windows() {
        let groups = parse_summary(&summary());
        assert_eq!(groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Gemini", "Claude and GPT"]);
        assert_eq!((used(&groups[0].five_hour), used(&groups[0].weekly)), (9, 18));
        assert_eq!((used(&groups[1].five_hour), used(&groups[1].weekly)), (27, 36));
        let gemini_5h = groups[0].five_hour.as_ref().unwrap();
        assert_eq!(gemini_5h.resets_at, Some(1_781_523_574));
        assert_eq!(gemini_5h.window_minutes, Some(300));
        assert_eq!(groups[1].weekly.as_ref().unwrap().window_minutes, Some(10_080));
    }

    #[test]
    fn accepts_older_and_snake_case_shapes() {
        let older = json!({ "groups": [{ "displayName": "Gemini Models", "buckets": [
            { "bucketId": "gemini-5h", "remainingFraction": 0.5 } ] }] });
        assert_eq!(used(&parse_summary(&older)[0].five_hour), 50);
        let snake = json!({ "summary": { "groups": [{ "display_name": "Gemini Models", "buckets": [
            { "bucket_id": "gemini-weekly", "remaining": { "remaining_fraction": 0.25 }, "reset_time": 1_781_523_574 } ] }] } });
        let groups = parse_summary(&snake);
        assert_eq!(used(&groups[0].weekly), 75);
        assert_eq!(groups[0].weekly.as_ref().unwrap().resets_at, Some(1_781_523_574));
    }

    #[test]
    fn never_guesses_unknown_or_missing_values() {
        let value = json!({ "groups": [{ "displayName": "Gemini Models", "buckets": [
            { "bucketId": "gemini-weekly", "displayName": "Weekly Limit" },
            { "bucketId": "gemini-5h", "disabled": true, "remainingFraction": 0.5 },
            { "bucketId": "gemini-daily", "displayName": "Daily", "remainingFraction": 0.5 }
        ]}]});
        assert!(parse_summary(&value).is_empty(), "no fraction, disabled, or unknown window: nothing to show");
        assert!(parse_summary(&json!({ "userStatus": {} })).is_empty());
    }

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn recognizes_antigravitys_language_server() {
        let names = vec!["language_server_windows_x64.exe".to_string()];
        let line = r"C:\Users\me\AppData\Local\Programs\Antigravity\resources\app\extensions\antigravity\bin\language_server_windows_x64.exe --app_data_dir antigravity --csrf_token 0b7f-9c --extension_server_port 52611";
        let c = candidate(42, &names, &args(line)).unwrap();
        assert_eq!((c.pid, c.token.expose(), c.extension_port, c.ide), (42, "0b7f-9c", Some(52611), false));

        let ide = candidate(43, &names, &args("language_server --app_data_dir=antigravity-ide --csrf_token=\"t1\"")).unwrap();
        assert_eq!((ide.token.expose(), ide.ide), ("t1", true));

        // Another editor's server, a server without a token, and other processes are ignored.
        assert!(candidate(1, &names, &args("language_server --app_data_dir windsurf --csrf_token x")).is_none());
        assert!(candidate(1, &names, &args("language_server --app_data_dir antigravity")).is_none());
        assert!(candidate(1, &["code.exe".into()], &args("code.exe --app_data_dir antigravity --csrf_token x")).is_none());
    }

    #[test]
    fn token_never_appears_in_debug_output() {
        let c = candidate(1, &["language_server".into()], &args("language_server --app_data_dir antigravity --csrf_token SECRET")).unwrap();
        assert!(!format!("{c:?}").contains("SECRET"));
    }

    /// Live check: finds a running Antigravity language server (or a stand-in with
    /// the same name and flags), reads its quota, then reads again through the
    /// remembered endpoint. Run with `cargo test -- --ignored live_` while one runs.
    #[test]
    #[ignore]
    fn live_reads_a_running_language_server() {
        let first = read(true).expect("a running Antigravity language server");
        assert!(!first.groups.is_empty());
        assert!(LAST_ENDPOINT.lock().unwrap().is_some());
        let second = read(true).expect("the remembered endpoint answers again");
        assert_eq!(first.groups, second.groups);
        for g in &first.groups {
            eprintln!("{}: 5h {:?}% used, weekly {:?}% used", g.name, g.five_hour.as_ref().map(|w| w.used_percent), g.weekly.as_ref().map(|w| w.used_percent));
        }
    }

    #[test]
    fn finds_listening_ports() {
        let netstat = "\
  Proto  Local Address          Foreign Address        State           PID
  TCP    127.0.0.1:52610        0.0.0.0:0              LISTENING       4242
  TCP    127.0.0.1:52611        0.0.0.0:0              ABHÖREN         4242
  TCP    127.0.0.1:52612        127.0.0.1:52700        ESTABLISHED     4242
  TCP    0.0.0.0:80             0.0.0.0:0              LISTENING       4
  TCP    [::1]:52613            [::]:0                 LISTENING       4242
";
        assert_eq!(parse_netstat(netstat, 4242), [52610, 52611, 52613]);
        let lsof = "\
COMMAND     PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
language_ 4242 me   12u  IPv4 0x1      0t0  TCP 127.0.0.1:52610 (LISTEN)
language_ 4242 me   13u  IPv4 0x2      0t0  TCP 127.0.0.1:52611 (LISTEN)
";
        assert_eq!(parse_lsof(lsof), [52610, 52611]);
    }
}
