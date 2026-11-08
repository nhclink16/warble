//! Finding sounds online: Myinstants (its search page, read as HTML) and Freesound (its API, with
//! the user's own key) behind one search, plus a few good sites to browse. Everything here
//! blocks, so the app calls it from a worker thread.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use crate::settings::AUDIO_EXTS;

pub const FREESOUND_KEY_URL: &str = "https://freesound.org/apiv2/apply";
const MYINSTANTS: &str = "https://www.myinstants.com";
const FREESOUND_SEARCH: &str = "https://freesound.org/apiv2/search/text/";
/// Results kept from each site.
const PER_SITE: usize = 12;
/// The biggest sound file a result may download.
const MAX_SOUND_BYTES: u64 = 20 << 20;

/// A site worth browsing, with what its licence means for a soundboard.
pub struct Site {
    pub name: &'static str,
    pub url: &'static str,
    pub note: &'static str,
}

pub const SITES: [Site; 4] = [
    Site {
        name: "Myinstants",
        url: "https://www.myinstants.com/en/index/us/",
        note: "Meme sounds and short clips. People upload them, so rights vary.",
    },
    Site {
        name: "Freesound",
        url: "https://freesound.org/",
        note: "Creative Commons sounds. Check each licence: some ask for credit.",
    },
    Site {
        name: "Pixabay sound effects",
        url: "https://pixabay.com/sound-effects/",
        note: "Free to use without credit, under the Pixabay\u{a0}licence.",
    },
    Site {
        name: "BBC Sound Effects",
        url: "https://sound-effects.bbcrewind.co.uk/",
        note: "Free for personal use only (the RemArc\u{a0}licence).",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Myinstants,
    Freesound,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Myinstants => "Myinstants",
            Source::Freesound => "Freesound",
        }
    }
}

/// One search result.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub name: String,
    pub source: Source,
    /// The mp3 to preview and add; also the result's id.
    pub url: String,
    pub seconds: Option<f32>,
    /// Freesound's licence, short: "CC0", "CC BY", "CC BY-NC".
    pub licence: Option<&'static str>,
}

/// What a search found, and a short line about any site that didn't answer.
#[derive(Debug, Default)]
pub struct Found {
    pub hits: Vec<Hit>,
    pub problem: Option<String>,
}

/// Why a site gave nothing.
#[derive(Debug, PartialEq)]
enum Fail {
    Offline,
    Said(String),
}

/// Searches Myinstants, and Freesound too when there's a key, at the same time. Results
/// alternate between the two so both show near the top.
pub fn search(query: &str, freesound_key: &str) -> Found {
    let (mi, fs) = std::thread::scope(|scope| {
        let fs = (!freesound_key.is_empty())
            .then(|| scope.spawn(|| search_freesound(query, freesound_key)));
        let mi = search_myinstants(query);
        let fs = fs.map(|h| h.join().unwrap_or(Err(Fail::Offline)));
        (mi, fs)
    });
    let problem = match (&mi, &fs) {
        (Err(Fail::Offline), Some(Err(Fail::Offline))) => {
            Some("Couldn’t reach Myinstants or Freesound. Check your internet.".into())
        }
        (Err(e), _) => Some(say(e, Source::Myinstants)),
        (_, Some(Err(e))) => Some(say(e, Source::Freesound)),
        _ => None,
    };
    let (mut mi, mut fs) = (
        mi.unwrap_or_default().into_iter(),
        fs.and_then(Result::ok).unwrap_or_default().into_iter(),
    );
    let mut hits = Vec::new();
    loop {
        let (a, b) = (mi.next(), fs.next());
        if a.is_none() && b.is_none() {
            break;
        }
        hits.extend(a.into_iter().chain(b));
    }
    Found { hits, problem }
}

fn say(fail: &Fail, site: Source) -> String {
    match fail {
        Fail::Offline => format!("Couldn’t reach {}. Check your internet.", site.name()),
        Fail::Said(why) => why.clone(),
    }
}

/// One shared client: short timeouts, statuses as values. On Windows it uses the system's own
/// TLS and certificate store.
pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::Agent::config_builder()
            .user_agent(concat!("Warble/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            .http_status_as_error(false);
        #[cfg(windows)]
        let config = config.tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        );
        config.build().new_agent()
    })
}

fn search_myinstants(query: &str) -> Result<Vec<Hit>, Fail> {
    let mut response = agent()
        .get(format!("{MYINSTANTS}/en/search/"))
        .query("name", query)
        .call()
        .map_err(|_| Fail::Offline)?;
    // A search with no results comes back as a 404 page saying so.
    if !matches!(response.status().as_u16(), 200 | 404) {
        return Err(Fail::Offline);
    }
    let html = response
        .body_mut()
        .read_to_string()
        .map_err(|_| Fail::Offline)?;
    let hits = parse_myinstants(&html);
    if hits.is_empty() && html.contains("class=\"small-button\"") {
        // Sound buttons we can't read: Myinstants changed its page.
        return Err(Fail::Said(
            "Couldn’t read Myinstants’ results. Its page may have changed.".into(),
        ));
    }
    Ok(hits)
}

/// Every sound button on a Myinstants search page: `onclick="play('/media/sounds/x.mp3', …)"`,
/// named by the link that follows it.
pub fn parse_myinstants(html: &str) -> Vec<Hit> {
    const PLAY: &str = "onclick=\"play('";
    let mut hits = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(PLAY) {
        rest = &rest[at + PLAY.len()..];
        let Some(end) = rest.find('\'') else { break };
        let path = &rest[..end];
        // This button's own block: up to the next one.
        let block = &rest[..rest.find(PLAY).unwrap_or(rest.len())];
        let name = block
            .find("instant-link")
            .and_then(|i| block[i..].find('>').map(|j| &block[i + j + 1..]))
            .and_then(|t| t.find("</a>").map(|j| unescape(t[..j].trim())));
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            continue;
        };
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{MYINSTANTS}{path}")
        };
        if !hits.iter().any(|h: &Hit| h.url == url) {
            hits.push(Hit {
                name,
                source: Source::Myinstants,
                url,
                seconds: None,
                licence: None,
            });
        }
        if hits.len() == PER_SITE {
            break;
        }
    }
    hits
}

/// The few HTML entities names use: `&amp;`, `&quot;`, `&#39;`, `&#x27;` and the like.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest
            .char_indices()
            .take(10)
            .find(|&(_, c)| c == ';')
            .map(|(i, _)| i)
        else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn search_freesound(query: &str, key: &str) -> Result<Vec<Hit>, Fail> {
    let mut response = agent()
        .get(FREESOUND_SEARCH)
        .query("query", query)
        .query("fields", "name,previews,duration,license")
        .query("filter", "duration:[0 TO 300]")
        .query("page_size", PER_SITE.to_string())
        .header("Authorization", format!("Token {key}"))
        .call()
        .map_err(|_| Fail::Offline)?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => {
            return Err(Fail::Said(
                "Freesound didn’t accept your key. Check it, or make a new one.".into(),
            ));
        }
        429 => {
            return Err(Fail::Said(
                "Freesound is busy. Try again in a minute.".into(),
            ));
        }
        _ => return Err(Fail::Offline),
    }
    let json = response
        .body_mut()
        .read_to_string()
        .map_err(|_| Fail::Offline)?;
    parse_freesound(&json).ok_or(Fail::Offline)
}

/// A Freesound text search's results, by their preview mp3s.
pub fn parse_freesound(json: &str) -> Option<Vec<Hit>> {
    #[derive(Deserialize)]
    struct Page {
        results: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        name: String,
        previews: Previews,
        duration: Option<f32>,
        #[serde(default)]
        license: String,
    }
    #[derive(Deserialize)]
    struct Previews {
        #[serde(rename = "preview-hq-mp3")]
        hq_mp3: String,
    }
    let page: Page = serde_json::from_str(json).ok()?;
    Some(
        page.results
            .into_iter()
            .map(|item| Hit {
                name: tidy_freesound_name(&item.name),
                source: Source::Freesound,
                url: item.previews.hq_mp3,
                seconds: item.duration,
                licence: licence(&item.license),
            })
            .collect(),
    )
}

/// "bruh_sound_01.wav" as "bruh sound 01".
fn tidy_freesound_name(name: &str) -> String {
    let mut name = name.trim();
    if let Some((stem, ext)) = name.rsplit_once('.')
        && ["wav", "mp3", "aif", "aiff", "flac", "ogg", "m4a"]
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    {
        name = stem;
    }
    let name = name.replace('_', " ");
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        "Sound".into()
    } else {
        name
    }
}

/// A Creative Commons licence URL, short.
fn licence(url: &str) -> Option<&'static str> {
    let url = url.to_lowercase();
    if url.contains("publicdomain/zero") {
        Some("CC0")
    } else if url.contains("/by-nc") {
        Some("CC BY-NC")
    } else if url.contains("/by/") || url.ends_with("/by") {
        Some("CC BY")
    } else if url.contains("sampling+") {
        Some("Sampling+")
    } else {
        None
    }
}

/// A result's file in a temporary folder, named after it so the soundboard copies it in under
/// that name. Downloaded once per result; later calls reuse it.
pub fn download(hit: &Hit) -> Result<PathBuf, String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hit.url.hash(&mut hasher);
    let dir = std::env::temp_dir()
        .join("warble-downloads")
        .join(format!("{:016x}", hasher.finish()));
    let path = dir.join(format!("{}.{}", safe_name(&hit.name), extension(&hit.url)));
    if !path.is_file() {
        let offline = || {
            format!(
                "Couldn’t download it from {}. Check your internet, then try again.",
                hit.source.name()
            )
        };
        std::fs::create_dir_all(&dir).map_err(|_| offline())?;
        fetch_to(&hit.url, &path, MAX_SOUND_BYTES).map_err(|_| offline())?;
    }
    Ok(path)
}

/// Streams `url` into `dest` through a ".part" file, so a broken download never looks done.
/// Refuses more than `limit` bytes.
pub fn fetch_to(url: &str, dest: &Path, limit: u64) -> std::io::Result<()> {
    let mut response = agent().get(url).call().map_err(std::io::Error::other)?;
    if response.status().as_u16() != 200 {
        return Err(std::io::Error::other(format!("HTTP {}", response.status())));
    }
    let part = dest.with_extension("part");
    let result = (|| {
        let mut file = std::fs::File::create(&part)?;
        let mut body = response.body_mut().with_config().limit(limit).reader();
        std::io::copy(&mut body, &mut file)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&part, dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// The audio extension at the end of a URL's path, else "mp3".
fn extension(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| AUDIO_EXTS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
        .unwrap_or("mp3")
}

/// A name that's safe as a Windows file name: no `<>:"/\|?*` or control characters, no
/// trailing dots or spaces, no reserved device names, and at most 60 characters.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut out = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((i, _)) = out.char_indices().nth(60) {
        out.truncate(i);
    }
    let out = out.trim_end_matches(['.', ' ']).trim_start_matches('.');
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|r| out.eq_ignore_ascii_case(r))
        || (out.len() == 4
            && out.is_ascii()
            && (out[..3].eq_ignore_ascii_case("COM") || out[..3].eq_ignore_ascii_case("LPT"))
            && out.as_bytes()[3].is_ascii_digit());
    match out {
        "" => "Sound".into(),
        _ if reserved => format!("{out} sound"),
        _ => out.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_saved_myinstants_page() {
        let hits = parse_myinstants(include_str!("fixtures/myinstants-search.html"));
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "BRUH",
                "BRUH sound effect!",
                "omg bruh oh hell nah",
                "HorrorMemeMusic",
                "Tom and Jerry Don't you believe it",
                "T&j grito do Tom",
            ]
        );
        assert_eq!(
            hits[0].url,
            "https://www.myinstants.com/media/sounds/movie_1.mp3"
        );
        assert!(hits.iter().all(|h| h.source == Source::Myinstants));
        assert!(parse_myinstants("<h2>No results for \"zzz\"</h2>").is_empty());
    }

    #[test]
    fn reads_freesound_results() {
        let json = r#"{"count": 2, "next": null, "results": [
            {"id": 1, "name": "bruh_sound_01.wav", "duration": 1.25,
             "license": "http://creativecommons.org/publicdomain/zero/1.0/",
             "previews": {"preview-hq-mp3": "https://cdn.freesound.org/previews/1/1-hq.mp3",
                          "preview-lq-mp3": "https://cdn.freesound.org/previews/1/1-lq.mp3"}},
            {"id": 2, "name": "Bruh", "duration": 0.8,
             "license": "https://creativecommons.org/licenses/by/4.0/",
             "previews": {"preview-hq-mp3": "https://cdn.freesound.org/previews/2/2-hq.mp3"}}
        ]}"#;
        let hits = parse_freesound(json).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "bruh sound 01");
        assert_eq!(hits[0].url, "https://cdn.freesound.org/previews/1/1-hq.mp3");
        assert_eq!(
            (hits[0].seconds, hits[0].licence),
            (Some(1.25), Some("CC0"))
        );
        assert_eq!(hits[1].licence, Some("CC BY"));
        assert_eq!(
            licence("http://creativecommons.org/licenses/by-nc/4.0/"),
            Some("CC BY-NC")
        );
        assert!(parse_freesound(r#"{"detail": "Invalid token."}"#).is_none());
    }

    #[test]
    fn names_are_safe_on_windows() {
        assert_eq!(safe_name("Air horn: the remix?"), "Air horn the remix");
        assert_eq!(safe_name("  ..what.. "), "what");
        assert_eq!(safe_name("a/b\\c|d*e\"f<g>h"), "a b c d e f g h");
        assert_eq!(safe_name("CON"), "CON sound");
        assert_eq!(safe_name("com1"), "com1 sound");
        assert_eq!(safe_name("abé"), "abé", "not cut inside a letter");
        assert_eq!(safe_name(&"é".repeat(70)).chars().count(), 60);
        assert_eq!(safe_name("???"), "Sound");
        assert_eq!(safe_name(&"x".repeat(100)).len(), 60);
        assert_eq!(extension("https://a.b/x/clip.MP3?v=1"), "MP3");
        assert_eq!(extension("https://a.b/x/clip"), "mp3");
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(
            unescape("Tom &amp; Jerry &#x27;n&#39; &quot;x&quot;"),
            "Tom & Jerry 'n' \"x\""
        );
        assert_eq!(unescape("AT&T & co &bogus;"), "AT&T & co &bogus;");
        assert_eq!(
            unescape("&ééééééééé;"),
            "&ééééééééé;",
            "not cut inside a letter"
        );
    }

    /// Live: `cargo test -p warble -- --ignored live_search`. Freesound too with FREESOUND_KEY.
    #[test]
    #[ignore = "needs the internet"]
    fn live_search_finds_bruh() {
        let key = std::env::var("FREESOUND_KEY").unwrap_or_default();
        let found = search("bruh", &key);
        assert_eq!(found.problem, None);
        let mi: Vec<&Hit> = found
            .hits
            .iter()
            .filter(|h| h.source == Source::Myinstants)
            .collect();
        eprintln!(
            "Myinstants: {:?}",
            mi.iter().map(|h| &h.name).collect::<Vec<_>>()
        );
        assert!(!mi.is_empty());
        let fs = found.hits.len() - mi.len();
        eprintln!(
            "Freesound: {fs} results{}",
            if key.is_empty() {
                " (no key, skipped)"
            } else {
                ""
            }
        );
        if !key.is_empty() {
            assert!(fs > 0);
        }
        let path = download(mi[0]).expect("downloads");
        let clip = warble_engine::Clip::decode(&path, 300).expect("decodes");
        eprintln!(
            "{}: {:.1} s at {} Hz",
            path.display(),
            clip.seconds(),
            clip.rate()
        );
        assert!(clip.seconds() > 0.1);
    }
}
