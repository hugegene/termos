//! Upgrading an installed `tos`: `tos update`, and the server's daily check for a newer release
//! (docs/plans/2026-11-10-tos-update.md). Both use `curl` (else `wget`) rather than an HTTP library, and the
//! installer stays the one place that knows how to install.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};

use crate::paths;

const RELEASES: &str = "https://github.com/hugegene/termos/releases";
/// At most one request a day, across every session on the machine (the result is kept in a file).
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often the server's thread looks at that file.
const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);

/// Where releases live. `TOS_RELEASES_URL` replaces it, for tests.
fn releases_url() -> String {
    env::var("TOS_RELEASES_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_owned())
        .unwrap_or_else(|| RELEASES.into())
}

/// "v0.2.10" → [0, 2, 10]. Anything after `-` or `+` is ignored.
fn parse(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches('v').split(['-', '+']).next()?;
    core.split('.').map(|part| part.parse().ok()).collect()
}

/// Whether `latest` is a newer version than `current`. Versions that don't parse are never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// The version in a release URL such as `…/releases/tag/v0.2.2`.
fn version_from_tag_url(url: &str) -> Option<String> {
    let tag = url.trim().rsplit_once("/tag/")?.1.trim_end_matches('/');
    parse(tag).map(|_| tag.trim_start_matches('v').to_owned())
}

/// The newest release's version: `<releases>/latest` redirects to its tag, so one request answers it without
/// following the redirect. `None` on any failure (offline, no curl or wget, no releases).
fn fetch_latest() -> Option<String> {
    let url = format!("{}/latest", releases_url());
    let curl = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "--max-time", "10", "-w", "%{redirect_url}", &url])
        .stderr(Stdio::null())
        .output();
    if let Ok(out) = curl
        && out.status.success()
    {
        return version_from_tag_url(&String::from_utf8_lossy(&out.stdout));
    }
    // wget prints the response headers on stderr; with no redirects allowed it stops at the Location.
    let out = Command::new("wget")
        .args(["-S", "--spider", "--max-redirect=0", "--timeout=10", &url])
        .stdout(Stdio::null())
        .output()
        .ok()?;
    version_from_wget(&String::from_utf8_lossy(&out.stderr))
}

/// The version in the first `Location:` header of wget's `-S` output.
fn version_from_wget(stderr: &str) -> Option<String> {
    stderr.lines().find_map(|line| {
        let line = line.trim();
        let value = line.get(..9).filter(|p| p.eq_ignore_ascii_case("location:")).map(|_| &line[9..])?;
        version_from_tag_url(value.split_whitespace().next()?)
    })
}

/// `<state>/update-check` holds "<unix seconds> <version>" from the last check ("-" if it failed).
fn check_file() -> PathBuf {
    paths::state_dir().join("update-check")
}

fn read_check(path: &Path) -> Option<(SystemTime, Option<String>)> {
    let text = fs::read_to_string(path).ok()?;
    let (secs, version) = text.trim().split_once(' ')?;
    let when = UNIX_EPOCH + Duration::from_secs(secs.parse().ok()?);
    Some((when, (version != "-").then(|| version.to_owned())))
}

/// The newest release's version, asking GitHub only if the last check is more than a day old.
fn latest_version() -> Option<String> {
    let path = check_file();
    if let Some((when, version)) = read_check(&path)
        && when.elapsed().is_ok_and(|age| age < CHECK_EVERY)
    {
        return version;
    }
    let version = fetch_latest();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = fs::write(&path, format!("{now} {}\n", version.as_deref().unwrap_or("-")));
    version
}

/// The server's background check: hourly it looks at the last check (asking GitHub at most once a day), and
/// calls `notify` with a newer version once per version. `TOS_NO_UPDATE_CHECK` (any value but "0" or empty)
/// turns it off.
pub fn spawn_check(notify: impl Fn(String) + Send + 'static) {
    if env::var("TOS_NO_UPDATE_CHECK").is_ok_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    thread::spawn(move || {
        let mut told: Option<String> = None;
        loop {
            if let Some(latest) = latest_version()
                && is_newer(&latest, env!("CARGO_PKG_VERSION"))
                && told.as_ref() != Some(&latest)
            {
                notify(latest.clone());
                told = Some(latest);
            }
            thread::sleep(LOOK_EVERY);
        }
    });
}

/// The status bar's text for a newer release.
pub fn notice(version: &str) -> String {
    format!("tos {version} is available: run tos update")
}

/// The first release with `tos update`: installing an older one leaves only the one-line installer to come back.
const FIRST_WITH_UPDATE: &str = "0.3.0";
/// The first release called `tos` (before it, `tb`, with differently named downloads).
const FIRST_AS_TOS: &str = "0.2.0";

/// Why `tos update --version <version>` shouldn't go ahead, if it shouldn't. Without `force`, that's any
/// release older than `tos update` itself, and releases from before the rename are never possible.
fn refuse_version(version: &str, force: bool) -> Option<String> {
    if parse(version).is_none() {
        return Some(format!("'{version}' isn't a version number, such as 0.3.0"));
    }
    if is_newer(FIRST_AS_TOS, version) {
        return Some(format!("releases before {FIRST_AS_TOS} were called tb, so `tos update` can't install {version}"));
    }
    if !force && is_newer(FIRST_WITH_UPDATE, version) {
        return Some(format!(
            "tos {version} doesn't have `tos update`, so after installing it only the one-line installer can \
             bring you forward again:\n  curl -fsSL {}/latest/download/install.sh | sh\nTo go ahead anyway: \
             tos update --version {version} --force",
            releases_url()
        ));
    }
    None
}

/// `tos update [--version X] [--force]`: downloads the newest installer and runs it. It puts the release in the
/// versions folder and points the `tos` symlink at it. Only a `tos` the installer put there is replaced: a build
/// from source is left alone. Without `--version`, nothing happens when this is already the newest release.
pub fn run(version: Option<String>, force: bool) -> anyhow::Result<()> {
    let version = version.map(|v| v.trim_start_matches('v').to_owned());
    if let Some(why) = version.as_deref().and_then(|v| refuse_version(v, force)) {
        bail!(why);
    }
    let exe = env::current_exe().and_then(|p| p.canonicalize()).context("finding the tos program")?;
    let versions = paths::data_dir().join("versions");
    let installed = versions.canonicalize().is_ok_and(|v| exe.parent().and_then(Path::parent) == Some(v.as_path()));
    if !installed {
        bail!(
            "this tos ({}) wasn't installed by the installer, so `tos update` won't replace it. To install the \
             latest release:\n  curl -fsSL {}/latest/download/install.sh | sh",
            exe.display(),
            releases_url()
        );
    }
    let download_base = env::var("TOS_DOWNLOAD_URL").ok().filter(|b| !b.is_empty());
    // Asking for the newest release is one request (the same as the daily check). It's skipped when the files
    // come from TOS_DOWNLOAD_URL, which has no "latest", and if it fails the installer finds out instead.
    if version.is_none()
        && download_base.is_none()
        && let Some(latest) = fetch_latest()
    {
        let current = env!("CARGO_PKG_VERSION");
        if !is_newer(&latest, current) {
            println!(
                "tos {current} is already the newest release ({latest}). To install it again: \
                 tos update --version {current}"
            );
            return Ok(());
        }
    }
    let script_url = match &download_base {
        Some(base) => format!("{}/install.sh", base.trim_end_matches('/')),
        None => format!("{}/latest/download/install.sh", releases_url()),
    };
    paths::ensure_private_dir(&paths::state_dir())?;
    let script = paths::state_dir().join(format!("install-{}.sh", std::process::id()));
    let fetched = download(&script_url, &script);
    let status = fetched.and_then(|()| {
        let mut sh = Command::new("sh");
        sh.arg(&script);
        // Where the installer put the `tos` symlink last time.
        if let Ok(dir) = fs::read_to_string(paths::data_dir().join("install-dir")) {
            sh.env("TOS_INSTALL_DIR", dir.trim());
        }
        if let Some(v) = &version {
            sh.env("TOS_VERSION", v);
        }
        sh.status().context("running the installer")
    });
    let _ = fs::remove_file(&script);
    if !status?.success() {
        bail!("the installer stopped (its message is above); the installed tos is unchanged");
    }
    Ok(())
}

fn download(url: &str, to: &Path) -> anyhow::Result<()> {
    let curl = Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(to).arg(url).status();
    let ok = match curl {
        Ok(status) => status.success(),
        Err(_) => Command::new("wget")
            .args(["-q", "-O"])
            .arg(to)
            .arg(url)
            .status()
            .context("tos update needs curl or wget")?
            .success(),
    };
    if !ok {
        bail!("couldn't download {url}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(is_newer("v0.3.0", "0.2.1"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("0.3.0-beta", "0.3.0"));
        assert!(!is_newer("garbage", "0.2.1"));
        assert!(!is_newer("0.3.0", "dev"));
    }

    #[test]
    fn old_versions_need_force_and_tb_ones_are_refused() {
        assert!(refuse_version("0.3.0", false).is_none());
        assert!(refuse_version("0.4.1", false).is_none());
        let why = refuse_version("0.2.0", false).unwrap();
        assert!(why.contains("doesn't have `tos update`") && why.contains("--force"), "{why}");
        assert!(refuse_version("0.2.0", true).is_none());
        assert!(refuse_version("0.1.1", true).unwrap().contains("called tb"));
        assert!(refuse_version("latest", false).unwrap().contains("isn't a version number"));
    }

    #[test]
    fn tag_urls_give_versions() {
        let base = "https://github.com/hugegene/termos/releases";
        assert_eq!(version_from_tag_url(&format!("{base}/tag/v0.2.2")).as_deref(), Some("0.2.2"));
        assert_eq!(version_from_tag_url(&format!("{base}/tag/v0.2.2/\n")).as_deref(), Some("0.2.2"));
        assert_eq!(version_from_tag_url(""), None);
        assert_eq!(version_from_tag_url(&format!("{base}/latest")), None);
        assert_eq!(version_from_tag_url(&format!("{base}/tag/nightly")), None);
    }

    #[test]
    fn wget_headers_give_versions() {
        // wget 1.21 on Ubuntu 24.04, asking GitHub on 2026-11-10.
        let stderr = "Spider mode enabled. Check if remote file exists.\n\
                      --2026-11-10 16:20:01--  https://github.com/hugegene/termos/releases/latest\n\
                      HTTP request sent, awaiting response... \n  HTTP/1.1 302 Found\n\
                      \x20 Location: https://github.com/hugegene/termos/releases/tag/v0.2.0\n\
                      Location: https://github.com/hugegene/termos/releases/tag/v0.2.0 [following]\n\
                      0 redirections exceeded.\n";
        assert_eq!(version_from_wget(stderr).as_deref(), Some("0.2.0"));
        assert_eq!(version_from_wget("  HTTP/1.1 404 Not Found\n"), None);
    }

    #[test]
    fn the_check_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check");
        fs::write(&path, "1762750000 0.2.2\n").unwrap();
        let (when, version) = read_check(&path).unwrap();
        assert_eq!(when, UNIX_EPOCH + Duration::from_secs(1_762_750_000));
        assert_eq!(version.as_deref(), Some("0.2.2"));
        fs::write(&path, "1762750000 -\n").unwrap();
        assert_eq!(read_check(&path).unwrap().1, None);
        fs::write(&path, "junk").unwrap();
        assert!(read_check(&path).is_none());
    }
}
