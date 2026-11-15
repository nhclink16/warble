//! Updates from Warble's public GitHub releases. Once a day Warble asks GitHub for the latest
//! release; when it's newer than this copy, a line above every page offers it. Update downloads
//! that release's installer and runs it silently with `/run`: the installer closes this Warble,
//! replaces it and opens the new one. Settings and sounds stay where they are. The installer is
//! a Windows one, so only Windows asks.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where releases are published.
const REPO: &str = "nhclink16/warble";
/// The first look waits this long after Warble starts, so it never slows the start down.
pub const FIRST_LOOK: Duration = Duration::from_secs(10);
/// Then it looks again this often while Warble runs.
pub const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The installer is about 7 MB; a download far past that isn't it.
const MAX_BYTES: u64 = 64 << 20;

/// A release newer than this copy, with its installer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// "0.2.0", from the tag "v0.2.0".
    pub version: String,
    /// `Warble-Setup-<version>.exe` among the release's files.
    pub url: String,
    pub size: u64,
}

/// Major, minor and patch of "0.2.0" or "v0.2.0". Anything else (a pre-release such as
/// "0.3.0-beta") is `None`, so it's never offered.
fn version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.strip_prefix('v').unwrap_or(v).split('.');
    let mut next = || parts.next()?.parse().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// The release in GitHub's "latest release" JSON, when it's newer than `current` and has its
/// installer, published from this repo.
pub fn newer(json: &str, current: &str) -> Option<Release> {
    let release: serde_json::Value = serde_json::from_str(json).ok()?;
    if release["draft"] != false || release["prerelease"] != false {
        return None;
    }
    let tag = release["tag_name"].as_str()?;
    if version(tag)? <= version(current)? {
        return None;
    }
    let number = tag.strip_prefix('v').unwrap_or(tag);
    let name = format!("Warble-Setup-{number}.exe");
    let asset = release["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"] == name.as_str())?;
    let url = asset["browser_download_url"].as_str()?;
    let ours = format!("https://github.com/{REPO}/releases/download/{tag}/{name}");
    (url == ours).then(|| Release {
        version: number.to_string(),
        url: ours,
        size: asset["size"].as_u64().unwrap_or(0),
    })
}

/// Asks GitHub for the latest release. `None` when there's nothing newer, or GitHub can't be
/// reached: it asks again next time.
pub fn check() -> Option<Release> {
    let mut response = crate::library::agent()
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let json = response
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .ok()?;
    newer(&json, env!("CARGO_PKG_VERSION"))
}

/// Downloads the release's installer into `dir`, and checks it's all there.
pub fn download(release: &Release, dir: &Path) -> Result<PathBuf, String> {
    fn failed<E>(_: E) -> String {
        "The update didn’t download. Check your internet, then try again.".into()
    }
    std::fs::create_dir_all(dir).map_err(failed)?;
    let path = dir.join(format!("Warble-Setup-{}.exe", release.version));
    crate::library::fetch_to(&release.url, &path, MAX_BYTES).map_err(failed)?;
    let size = std::fs::metadata(&path).map_err(failed)?.len();
    if size != release.size {
        let _ = std::fs::remove_file(&path);
        return Err(failed(()));
    }
    Ok(path)
}

/// Starts the downloaded installer silently. It closes this Warble, so the caller saves first.
pub fn install(installer: &Path) -> Result<(), String> {
    std::process::Command::new(installer)
        .args(["/S", "/run"])
        .spawn()
        .map(drop)
        .map_err(|_| "Warble couldn’t start the update. Try again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latest(tag: &str, asset: &str, url: &str) -> String {
        serde_json::json!({
            "tag_name": tag,
            "draft": false,
            "prerelease": false,
            "assets": [
                {"name": "notes.txt", "size": 1, "browser_download_url": "https://example.com/x"},
                {"name": asset, "size": 7336499, "browser_download_url": url},
            ],
        })
        .to_string()
    }

    const URL: &str = "https://github.com/nhclink16/warble/releases/download/v0.2.0/\
                       Warble-Setup-0.2.0.exe";

    #[test]
    fn offers_only_a_newer_release_with_its_installer() {
        let json = latest("v0.2.0", "Warble-Setup-0.2.0.exe", URL);
        assert_eq!(
            newer(&json, "0.1.0"),
            Some(Release {
                version: "0.2.0".into(),
                url: URL.into(),
                size: 7336499,
            })
        );
        // The same version or an older one: nothing to offer.
        assert_eq!(newer(&json, "0.2.0"), None);
        assert_eq!(newer(&json, "0.10.0"), None, "compared as numbers");
        assert!(
            newer(
                &latest(
                    "v0.10.0",
                    "Warble-Setup-0.10.0.exe",
                    &URL.replace("0.2.0", "0.10.0")
                ),
                "0.9.0"
            )
            .is_some()
        );
        // No installer, or one from anywhere but this repo's releases.
        assert_eq!(newer(&latest("v0.2.0", "Other.exe", URL), "0.1.0"), None);
        let elsewhere = URL.replace("nhclink16", "someone-else");
        assert_eq!(
            newer(
                &latest("v0.2.0", "Warble-Setup-0.2.0.exe", &elsewhere),
                "0.1.0"
            ),
            None
        );
        // Pre-releases, drafts, odd tags and odd JSON are never offered.
        let beta = latest("v0.3.0-beta", "Warble-Setup-0.3.0-beta.exe", URL);
        assert_eq!(newer(&beta, "0.1.0"), None);
        let draft = json.replace("\"draft\":false", "\"draft\":true");
        assert_eq!(newer(&draft, "0.1.0"), None);
        assert_eq!(newer("{}", "0.1.0"), None);
        assert_eq!(newer("not json", "0.1.0"), None);
    }

    #[test]
    fn versions_read_as_three_numbers() {
        assert_eq!(version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(version("0.10.0"), Some((0, 10, 0)));
        for odd in ["1.2", "1.2.3.4", "v1.2.x", "", "1.2.3-rc1"] {
            assert_eq!(version(odd), None, "{odd}");
        }
    }

    /// Live: the published latest release is offered to a 0.1.0 Warble as this version, and its
    /// installer downloads whole. `cargo test -p warble the_live_release -- --ignored`.
    #[test]
    #[ignore = "needs the internet and a published release"]
    fn the_live_release_is_offered_and_downloads() {
        let json = crate::library::agent()
            .get(format!(
                "https://api.github.com/repos/{REPO}/releases/latest"
            ))
            .header("Accept", "application/vnd.github+json")
            .call()
            .expect("asks GitHub")
            .body_mut()
            .read_to_string()
            .expect("reads the release");
        let release = newer(&json, "0.1.0").expect("offers the latest release");
        assert_eq!(release.version, env!("CARGO_PKG_VERSION"));
        let dir = std::env::temp_dir().join("warble-live-release");
        let file = download(&release, &dir).expect("downloads the installer");
        assert!(file.is_file());
        assert_eq!(std::fs::metadata(&file).unwrap().len(), release.size);
        eprintln!("{} ({} bytes)", file.display(), release.size);
    }
}
