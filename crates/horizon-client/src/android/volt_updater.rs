//! One-time ABI-matched Volt Launcher update reminder, plus the in-app
//! downloader behind its Update button.
//!
//! The APK is fetched here, byte by byte, into the profile's own folder and
//! then handed to the system installer through `PackageInstaller` (see
//! `HorizonActivity.installApk`), so updating no longer bounces the player out
//! to a browser. Volt therefore asks for `REQUEST_INSTALL_PACKAGES`; Android
//! still shows its own confirmation before anything is replaced.
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

const RELEASES_LIST_URL: &str =
    "https://api.github.com/repos/entitybrian69-bit/Volt-launcher/releases?per_page=15";
const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/entitybrian69-bit/Volt-launcher/releases/latest";
const SEEN_FILE: &str = "volt-seen-updates.json";

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub tag_name: String,
    pub display_version: String,
    pub abi: String,
    pub abi_label: String,
    pub asset_name: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub whats_new: Vec<String>,
}

pub struct UpdatePrompt {
    pub info: UpdateInfo,
    pub scroll: f32,
}

#[derive(Default, Serialize, Deserialize)]
struct SeenStore {
    #[serde(default)]
    seen_tags: Vec<String>,
}

impl SeenStore {
    fn load(home: &Path) -> Self {
        std::fs::read(home.join(SEEN_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn contains(&self, tag: &str) -> bool {
        self.seen_tags.iter().any(|t| t == tag)
    }

    fn mark_seen(&mut self, home: &Path, tag: &str) {
        if !self.contains(tag) {
            self.seen_tags.push(tag.to_owned());
            if self.seen_tags.len() > 64 {
                let drop_n = self.seen_tags.len() - 64;
                self.seen_tags.drain(0..drop_n);
            }
        }
        let part = home.join("volt-seen-updates.part");
        if let Ok(bytes) = serde_json::to_vec_pretty(self)
            && std::fs::write(&part, bytes).is_ok()
        {
            let _ = std::fs::rename(&part, home.join(SEEN_FILE));
        }
    }
}

pub fn mark_update_seen(home: &Path, tag: &str) {
    let mut store = SeenStore::load(home);
    store.mark_seen(home, tag);
}

pub fn select_device_abi(supported_abis: &[String]) -> (&'static str, &'static str) {
    // `Build.SUPPORTED_ABIS` is ordered by the device's preference, so the
    // first one we ship wins: an x86_64 emulator with ARM translation still
    // gets its native x86_64 APK, not the translated ARM one.
    for abi in supported_abis {
        if abi.eq_ignore_ascii_case("arm64-v8a") {
            return ("arm64-v8a", "64-bit ARM64");
        }
        if abi.eq_ignore_ascii_case("armeabi-v7a") || abi.eq_ignore_ascii_case("armeabi") {
            return ("armeabi-v7a", "32-bit ARMv7");
        }
        if abi.eq_ignore_ascii_case("x86_64") {
            return ("x86_64", "64-bit x86");
        }
        if abi.eq_ignore_ascii_case("x86") {
            return ("x86", "32-bit x86");
        }
    }
    if cfg!(target_arch = "aarch64") {
        ("arm64-v8a", "64-bit ARM64")
    } else if cfg!(target_arch = "x86_64") {
        ("x86_64", "64-bit x86")
    } else if cfg!(target_arch = "x86") {
        ("x86", "32-bit x86")
    } else {
        ("armeabi-v7a", "32-bit ARMv7")
    }
}

pub fn parse_semver(raw: &str) -> Option<(u32, u32, u32)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+', ' ']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}

fn tag_commit_suffix(tag: &str) -> Option<&str> {
    let (_, sha) = tag.rsplit_once("-build.")?;
    let clean = sha.trim();
    (!clean.is_empty()).then_some(clean)
}

pub fn is_newer_release(
    installed_version: &str,
    installed_commit: Option<&str>,
    remote_tag: &str,
) -> bool {
    let Some(inst) = parse_semver(installed_version) else {
        return false;
    };
    let Some(rem) = parse_semver(remote_tag) else {
        return false;
    };
    if rem > inst {
        return true;
    }
    if rem < inst {
        return false;
    }
    if let (Some(local_sha), Some(remote_sha)) = (
        installed_commit.map(str::trim).filter(|s| s.len() >= 7),
        tag_commit_suffix(remote_tag).filter(|s| s.len() >= 7),
    ) {
        return !local_sha.starts_with(remote_sha) && !remote_sha.starts_with(local_sha);
    }
    false
}

pub fn format_whats_new(body: &str, fallback_title: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in body.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with("---") || line.starts_with("```") {
            continue;
        }
        while let Some(rest) = line.strip_prefix('#') {
            line = rest.trim_start();
        }
        let cleaned = line.replace("**", "").replace('`', "");
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        if let Some(bullet) = cleaned
            .strip_prefix("- ")
            .or_else(|| cleaned.strip_prefix("* "))
            .or_else(|| cleaned.strip_prefix("• "))
        {
            out.push(format!("• {}", bullet.trim()));
        } else {
            out.push(cleaned.to_owned());
        }
        if out.len() >= 18 {
            break;
        }
    }
    if out.is_empty() {
        out.push(format!("• {fallback_title}"));
        out.push("• Performance, compatibility, and stability improvements.".into());
    }
    out
}

pub fn spawn_startup_check(home: PathBuf) -> mpsc::Receiver<UpdateInfo> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("volt-update-check".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(900));
            if let Some(info) = check_once_blocking(&home, false) {
                let _ = tx.send(info);
            }
        })
        .ok();
    rx
}

/// The About section's "Check for updates": always answers (`None` = up to
/// date), includes build prereleases, and ignores the seen store — the
/// player explicitly asked.
pub fn spawn_manual_check(home: PathBuf) -> mpsc::Receiver<Option<UpdateInfo>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("volt-update-check".into())
        .spawn(move || {
            let _ = tx.send(check_once_blocking(&home, true));
        })
        .ok();
    rx
}

/// Where a downloaded update is kept until the installer takes it.
pub const UPDATE_DIR: &str = "volt-updates";

/// One step of an in-app update download.
#[derive(Debug)]
pub enum DownloadUpdate {
    /// Bytes written so far, and the asset's size when GitHub announced it.
    Progress(u64, u64),
    /// The finished APK, handed to the installer next.
    Ready(PathBuf),
    Failed(String),
}

/// Fetches `info`'s APK into `home/volt-updates/` and reports progress as it
/// writes. The final message is either [`DownloadUpdate::Ready`] with the file
/// or [`DownloadUpdate::Failed`] with something a player can read.
pub fn spawn_download(home: PathBuf, info: UpdateInfo) -> mpsc::Receiver<DownloadUpdate> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("volt-update-download".into())
        .stack_size(1024 * 1024)
        .spawn(move || {
            let dir = home.join(UPDATE_DIR);
            let name = if info.asset_name.trim().is_empty() {
                "volt-update.apk".to_owned()
            } else {
                info.asset_name.clone()
            };
            let target = dir.join(&name);
            let send = |m| {
                let _ = tx.send(m);
            };
            if let Some(parent) = target.parent()
                && std::fs::create_dir_all(parent).is_err()
            {
                send(DownloadUpdate::Failed(
                    "Could not make room for the update on this device.".into(),
                ));
                return;
            }
            let agent = ureq::AgentBuilder::new()
                .https_only(true)
                .user_agent(&format!("VoltLauncher/{}", crate::backend::VERSION))
                .timeout_connect(Duration::from_secs(15))
                .timeout_read(Duration::from_secs(60))
                .build();
            let resp = match agent.get(&info.download_url).call() {
                Ok(resp) => resp,
                Err(e) => {
                    send(DownloadUpdate::Failed(format!(
                        "The update download could not start: {e}"
                    )));
                    return;
                }
            };
            let total = resp
                .header("content-length")
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(info.size_bytes);
            let mut reader = resp.into_reader();
            let mut file = match std::fs::File::create(&target) {
                Ok(file) => file,
                Err(e) => {
                    send(DownloadUpdate::Failed(format!(
                        "Could not save the update: {e}"
                    )));
                    return;
                }
            };
            let mut buffer = vec![0u8; 64 * 1024];
            let mut written: u64 = 0;
            let mut last_report: u64 = 0;
            loop {
                let read = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        let _ = std::fs::remove_file(&target);
                        send(DownloadUpdate::Failed(format!(
                            "The update download stopped partway: {e}"
                        )));
                        return;
                    }
                };
                if std::io::Write::write_all(&mut file, &buffer[..read]).is_err() {
                    let _ = std::fs::remove_file(&target);
                    send(DownloadUpdate::Failed(
                        "Could not write the update to this device's storage.".into(),
                    ));
                    return;
                }
                written += read as u64;
                if written - last_report >= 512 * 1024 {
                    last_report = written;
                    send(DownloadUpdate::Progress(written, total));
                }
            }
            let _ = std::io::Write::flush(&mut file);
            drop(file);
            if written == 0 || (total > 0 && written != total) {
                let _ = std::fs::remove_file(&target);
                send(DownloadUpdate::Failed(format!(
                    "The update download was incomplete ({written} of {total} bytes)."
                )));
                return;
            }
            send(DownloadUpdate::Progress(written, written));
            send(DownloadUpdate::Ready(target));
        })
        .ok();
    rx
}

/// Whether `rel` is a full release: not a prerelease, not a per-commit
/// `-build.` tag, and a real `vX.Y.Z` version — the rolling `debug` testing
/// release never qualifies. Only full releases may interrupt the player with
/// the startup update popup.
fn is_full_release(rel: &serde_json::Value, tag: &str) -> bool {
    rel.get("prerelease").and_then(|v| v.as_bool()) != Some(true)
        && tag_commit_suffix(tag).is_none()
        && parse_semver(tag).is_some()
}

fn check_once_blocking(home: &Path, manual: bool) -> Option<UpdateInfo> {
    let mut seen = SeenStore::load(home);
    let abis = crate::native_ime::device_supported_abis();
    let (target_abi, abi_label) = select_device_abi(&abis);
    let installed_ver = crate::native_ime::app_version_name();
    let installed_commit = option_env!("HORIZON_ANDROID_BUILD_COMMIT");

    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .user_agent(&format!(
            "VoltLauncher/{} (Android; {})",
            crate::backend::VERSION,
            target_abi
        ))
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(12))
        .build();

    let releases = fetch_releases(&agent)?;
    for rel in releases {
        if rel.get("draft").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let Some(tag) = rel.get("tag_name").and_then(|v| v.as_str()) else {
            continue;
        };
        // The automatic popup only ever interrupts for full releases; the
        // About section's manual check reports every build.
        if !manual && !is_full_release(&rel, tag) {
            continue;
        }
        if !is_newer_release(&installed_ver, installed_commit, tag) {
            continue;
        }
        if !manual && seen.contains(tag) {
            return None;
        }
        let Some(assets) = rel.get("assets").and_then(|v| v.as_array()) else {
            continue;
        };
        let matched = assets.iter().find_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".apk") || !lower.contains(target_abi) {
                return None;
            }
            let url = asset.get("browser_download_url")?.as_str()?;
            if !url.starts_with("https://") {
                return None;
            }
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            Some((name.to_owned(), url.to_owned(), size))
        });
        let Some((asset_name, download_url, size_bytes)) = matched else {
            continue;
        };
        let title = rel
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(tag);
        let body = rel.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let display_version = parse_semver(tag)
            .map(|(ma, mi, pa)| format!("v{ma}.{mi}.{pa}"))
            .unwrap_or_else(|| tag.to_owned());

        // Persist immediately so this release reminder only appears ONCE and never nags again!
        if !manual {
            seen.mark_seen(home, tag);
        }

        return Some(UpdateInfo {
            tag_name: tag.to_owned(),
            display_version,
            abi: target_abi.to_owned(),
            abi_label: abi_label.to_owned(),
            asset_name,
            download_url,
            size_bytes,
            whats_new: format_whats_new(body, title),
        });
    }
    None
}

fn fetch_releases(agent: &ureq::Agent) -> Option<Vec<serde_json::Value>> {
    if let Ok(resp) = agent
        .get(RELEASES_LIST_URL)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        let mut buf = Vec::new();
        if resp
            .into_reader()
            .take(512 * 1024)
            .read_to_end(&mut buf)
            .is_ok()
            && let Ok(serde_json::Value::Array(arr)) = serde_json::from_slice(&buf)
            && !arr.is_empty()
        {
            return Some(arr);
        }
    }
    let resp = agent
        .get(RELEASES_LATEST_URL)
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(256 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    let val: serde_json::Value = serde_json::from_slice(&buf).ok()?;
    Some(vec![val])
}
