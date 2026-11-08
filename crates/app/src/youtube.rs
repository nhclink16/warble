//! Audio from a YouTube or X link through yt-dlp, which Warble downloads on first use (from its
//! official GitHub releases) into its own `bin` folder and keeps up to date. Everything here
//! blocks, so the app calls it from a worker thread.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

#[cfg(windows)]
const TOOL: &str = "yt-dlp.exe";
#[cfg(target_os = "macos")]
const TOOL: &str = "yt-dlp_macos";
#[cfg(not(any(windows, target_os = "macos")))]
const TOOL: &str = "yt-dlp_linux";
const RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";
/// For the one-time notice before the download.
pub const TOOL_SIZE: &str = "about 24 MB";
/// The longest video it fetches.
pub const MAX_SECONDS: u64 = 20 * 60;
/// How often it asks yt-dlp to update itself.
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The way out when a link fails: dropped videos open in the trim editor.
const DROP_IT: &str = "save the video as an MP4 and drop it on Warble’s window on your PC.";
/// Audio Warble can't decode.
pub const UNREADABLE: &str = "Warble can’t read this video’s audio. Save the video as an MP4 and drop it on Warble’s window on your PC.";

/// One yt-dlp at a time: an update replaces the program a fetch would run.
static RUN: Mutex<()> = Mutex::new(());

/// Where Warble keeps yt-dlp, under its own folder (`%APPDATA%\Warble\bin` on Windows).
pub fn tool_path(dir: &Path) -> PathBuf {
    dir.join("bin").join(TOOL)
}

fn stamp(dir: &Path) -> PathBuf {
    dir.join("bin").join("yt-dlp.checked")
}

/// Downloads yt-dlp from its official releases.
pub fn install(dir: &Path) -> Result<(), String> {
    let _one = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let tool = tool_path(dir);
    let failed = |_| "The download didn’t finish. Check your internet, then try again.".to_string();
    std::fs::create_dir_all(tool.parent().unwrap_or(dir)).map_err(failed)?;
    crate::library::fetch_to(&format!("{RELEASES}{TOOL}"), &tool, 200 << 20).map_err(failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).map_err(failed)?;
    }
    let _ = std::fs::write(stamp(dir), b"");
    Ok(())
}

/// Lets yt-dlp update itself (`-U`) if it hasn't tried in the last day. YouTube changes often,
/// and an old yt-dlp stops working. Quiet: a failed update leaves the old one.
pub fn update_if_due(dir: &Path) {
    let tool = tool_path(dir);
    let due = std::fs::metadata(stamp(dir))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age > UPDATE_EVERY);
    if !tool.is_file() || !due {
        return;
    }
    let _one = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::write(stamp(dir), b"");
    let _ = command(&tool).arg("-U").output();
}

/// yt-dlp with no window, no input and its output captured.
fn command(tool: &Path) -> Command {
    let mut cmd = Command::new(tool);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// The download yt-dlp picks, best first: YouTube's m4a audio; an audio-only mp4 stream (X's);
/// for a post with only whole videos, an mp4 of at most 720p, whose AAC audio Warble reads out
/// of it; anything else last (Opus, which gets a plain error). None of them needs ffmpeg.
pub const FORMAT: &str =
    "bestaudio[ext=m4a]/bestaudio[ext=mp4]/best[ext=mp4][height<=720]/best[ext=mp4]/bestaudio";

/// What yt-dlp is asked to do: the audio (see `FORMAT`) of one video (not its playlist), at most
/// `MAX_SECONDS` long and not live, saved in `out` under its id. It prints the saved file's
/// path, then the title.
pub fn args(url: &str, out: &Path) -> Vec<String> {
    vec![
        "--no-playlist".into(),
        "--no-progress".into(),
        "--no-warnings".into(),
        "--encoding".into(),
        "utf-8".into(),
        "-f".into(),
        FORMAT.into(),
        "--match-filter".into(),
        format!("duration <= {MAX_SECONDS} & !is_live"),
        "-o".into(),
        out.join("%(id)s.%(ext)s").to_string_lossy().into_owned(),
        "--print".into(),
        "after_move:filepath".into(),
        "--print".into(),
        "after_move:title".into(),
        "--".into(),
        url.into(),
    ]
}

/// Whether `text` could be a link worth handing to yt-dlp.
pub fn looks_like_link(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("https://") || t.starts_with("http://"))
        && t.len() > 12
        && !t.contains(char::is_whitespace)
}

/// Fetches a video's audio into `out`: the file and the video's title (without the t.co links
/// an X post's text carries).
pub fn fetch(dir: &Path, url: &str, out: &Path) -> Result<(PathBuf, String), String> {
    let _one = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let failed = || format!("Couldn’t get the audio. Try again, or {DROP_IT}");
    std::fs::create_dir_all(out).map_err(|_| failed())?;
    let output = command(&tool_path(dir))
        .args(args(url.trim(), out))
        .output()
        .map_err(|_| failed())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(plain_error(&String::from_utf8_lossy(&output.stderr)));
    }
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let Some(at) = lines.iter().position(|l| Path::new(l).is_file()) else {
        // Nothing saved and no error: the length filter turned it down.
        return Err(format!(
            "That video is too long. Pick one under {} minutes.",
            MAX_SECONDS / 60
        ));
    };
    let file = PathBuf::from(lines[at]);
    let words: Vec<&str> = lines
        .get(at + 1)
        .map_or("", |t| t)
        .split_whitespace()
        .filter(|w| !w.starts_with("http"))
        .collect();
    let title = if words.is_empty() {
        "Sound".to_string()
    } else {
        words.join(" ")
    };
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ["webm", "opus"].iter().any(|e| e.eq_ignore_ascii_case(ext)) {
        let _ = std::fs::remove_file(&file);
        return Err(UNREADABLE.into());
    }
    Ok((file, title))
}

/// yt-dlp's error, in a short plain sentence that says what to do next.
fn plain_error(stderr: &str) -> String {
    let e = stderr.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
    if has(&["unsupported url", "is not a valid url"]) {
        "Warble can’t get audio from that link. Paste a link to a YouTube or X video.".into()
    } else if has(&["no video could be found"]) {
        "That post has no video. Paste a link to a post with one.".into()
    } else if has(&[
        "nsfw tweet requires authentication",
        "protected tweet",
        "you are not authorized",
    ]) {
        format!("That post needs a signed-in X account. Instead, {DROP_IT}")
    } else if has(&["could not retrieve guest token"]) {
        format!("X is blocking downloads right now. Try again later, or {DROP_IT}")
    } else if has(&[
        "requested tweet is unavailable",
        "tweetunavailable",
        "no longer exists",
    ]) {
        "That post isn’t available. Check the link.".into()
    } else if has(&["private video"]) {
        "That video is private. Try another video.".into()
    } else if has(&[
        "confirm your age",
        "age-restricted",
        "inappropriate for some users",
    ]) {
        format!("That video is age-restricted. Instead, {DROP_IT}")
    } else if has(&["not a bot"]) {
        format!("YouTube is blocking downloads right now. Try again later, or {DROP_IT}")
    } else if has(&[
        "video unavailable",
        "this video is not available",
        "has been removed",
    ]) {
        "That video isn’t available. Check the link.".into()
    } else if has(&["requested format is not available"]) {
        UNREADABLE.into()
    } else if has(&[
        "getaddrinfo",
        "name resolution",
        "timed out",
        "connection",
        "network",
    ]) {
        "Couldn’t reach the site. Check your internet.".into()
    } else {
        format!("Couldn’t get that video’s audio. Try again, or {DROP_IT}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_m4a_audio_of_one_short_video() {
        let out = Path::new("tmp").join("yt");
        let args = args("https://youtu.be/abc?list=xyz", &out);
        let after = |flag: &str| {
            let i = args.iter().position(|a| a == flag).unwrap();
            args[i + 1].as_str()
        };
        assert_eq!(after("-f"), FORMAT);
        assert!(
            FORMAT.starts_with("bestaudio[ext=m4a]/"),
            "YouTube's m4a first"
        );
        assert!(
            FORMAT.contains("/best[ext=mp4]"),
            "X posts with only whole videos"
        );
        assert_eq!(after("--match-filter"), "duration <= 1200 & !is_live");
        assert_eq!(after("-o"), out.join("%(id)s.%(ext)s").to_string_lossy());
        assert!(args.contains(&"--no-playlist".to_string()));
        let prints: Vec<&String> = args
            .iter()
            .zip(&args[1..])
            .filter(|(a, _)| *a == "--print")
            .map(|(_, b)| b)
            .collect();
        assert_eq!(prints, ["after_move:filepath", "after_move:title"]);
        // The link comes last, after "--", so it can never read as an option.
        assert_eq!(
            &args[args.len() - 2..],
            ["--", "https://youtu.be/abc?list=xyz"]
        );
    }

    #[test]
    fn links_and_errors_read_plainly() {
        assert!(looks_like_link(
            " https://www.youtube.com/watch?v=jNQXAC9IVRw "
        ));
        assert!(!looks_like_link("bruh"));
        assert!(!looks_like_link("https://x"));
        assert!(!looks_like_link("https://youtube.com/watch?v=a b"));
        assert_eq!(
            plain_error(
                "ERROR: [youtube] abc: Private video. Sign in if you've been granted access"
            ),
            "That video is private. Try another video."
        );
        assert_eq!(
            plain_error("ERROR: [youtube] abc: Video unavailable"),
            "That video isn’t available. Check the link."
        );
        assert_eq!(
            plain_error("ERROR: Unsupported URL: https://example.com/"),
            "Warble can’t get audio from that link. Paste a link to a YouTube or X video."
        );
        assert_eq!(
            plain_error("ERROR: [twitter] 123: No video could be found in this tweet"),
            "That post has no video. Paste a link to a post with one."
        );
        assert_eq!(
            plain_error("ERROR: [twitter] 123: NSFW tweet requires authentication."),
            "That post needs a signed-in X account. Instead, save the video as an MP4 and drop \
             it on Warble’s window on your PC."
        );
        assert_eq!(
            plain_error("ERROR: [twitter:broadcast] 1rmx: Broadcast no longer exists"),
            "That post isn’t available. Check the link."
        );
        assert_eq!(
            plain_error("ERROR: [youtube] abc: Sign in to confirm you're not a bot"),
            "YouTube is blocking downloads right now. Try again later, or save the video as an \
             MP4 and drop it on Warble’s window on your PC."
        );
        assert_eq!(
            plain_error("something odd"),
            "Couldn’t get that video’s audio. Try again, or save the video as an MP4 and drop it \
             on Warble’s window on your PC."
        );
        // What people read: plain words, curly apostrophes, no tool or codec names.
        for e in [
            "unsupported url",
            "no video could be found",
            "protected tweet",
            "could not retrieve guest token",
            "no longer exists",
            "private video",
            "age-restricted",
            "not a bot",
            "video unavailable",
            "requested format is not available",
            "timed out",
            "odd",
        ] {
            let said = plain_error(e);
            assert!(!said.contains('\''), "{said}");
            for word in ["yt-dlp", "Opus", "fetch"] {
                assert!(!said.contains(word), "{said}");
            }
        }
        assert!(tool_path(Path::new("cfg")).starts_with(Path::new("cfg").join("bin")));
    }

    /// Live: downloads the Linux yt-dlp into a temp folder, fetches "Me at the zoo" (19 s), and
    /// trims and saves two seconds of it. `cargo test -p warble -- --ignored live_youtube`.
    #[test]
    #[ignore = "needs the internet and runs yt-dlp"]
    fn live_youtube_fetch_trim_and_save() {
        let dir = std::env::temp_dir().join("warble-live-youtube");
        if !tool_path(&dir).is_file() {
            install(&dir).expect("installs yt-dlp");
        }
        update_if_due(&dir);
        let out = dir.join("out");
        let (file, title) =
            fetch(&dir, "https://www.youtube.com/watch?v=jNQXAC9IVRw", &out).expect("fetches");
        eprintln!("{title}: {}", file.display());
        assert_eq!(file.extension().and_then(|e| e.to_str()), Some("m4a"));
        let clip = warble_engine::Clip::decode(&file, MAX_SECONDS).expect("decodes m4a");
        eprintln!("{:.1} s at {} Hz", clip.seconds(), clip.rate());
        let mut trim = crate::trim::Trim::new(clip);
        trim.set_start(3.0);
        trim.set_end(5.0);
        let name = crate::trim::file_name(&title);
        let saved =
            crate::trim::save(&dir.join("sounds"), &name, &trim.clip, trim.range()).expect("saves");
        let back = warble_engine::Clip::decode(&saved, 300).unwrap();
        assert!((back.seconds() - 2.0).abs() < 0.01, "{}", back.seconds());
        eprintln!("saved {}", saved.display());
    }

    /// Live: an X post's audio, through `FORMAT` (its audio-only stream), and through the
    /// whole-video mp4 that posts without one fall back to. `cargo test -p warble -- --ignored
    /// live_x`. Shares the yt-dlp `live_youtube_fetch_trim_and_save` downloads.
    #[test]
    #[ignore = "needs the internet and runs yt-dlp"]
    fn live_x_post_audio_decodes() {
        let dir = std::env::temp_dir().join("warble-live-youtube");
        if !tool_path(&dir).is_file() {
            install(&dir).expect("installs yt-dlp");
        }
        let url = "https://x.com/historyinmemes/status/1790637656616943991";
        let out = dir.join("out-x");
        let (file, title) = fetch(&dir, url, &out).expect("fetches");
        eprintln!("{title}: {}", file.display());
        assert!(!title.contains("https://"), "{title}");
        let clip = warble_engine::Clip::decode(&file, MAX_SECONDS).expect("decodes");
        eprintln!(
            "audio-only stream: {:.1} s at {} Hz",
            clip.seconds(),
            clip.rate()
        );
        assert!(clip.seconds() > 10.0);

        let whole = command(&tool_path(&dir))
            .args(["--no-warnings", "-f", "best[ext=mp4][height<=720]", "-o"])
            .arg(out.join("whole.%(ext)s"))
            .args(["--print", "after_move:filepath", "--", url])
            .output()
            .unwrap();
        let path = String::from_utf8_lossy(&whole.stdout).trim().to_string();
        let video = warble_engine::Clip::decode(Path::new(&path), MAX_SECONDS).expect("decodes");
        eprintln!(
            "whole video {path}: {:.1} s at {} Hz",
            video.seconds(),
            video.rate()
        );
        assert!((video.seconds() - clip.seconds()).abs() < 0.5);
    }
}
