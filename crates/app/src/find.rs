//! The Find sounds page and the trim editor: their state and what they do. Searching,
//! downloading and yt-dlp run on worker threads; the results land back on the UI thread.

use rune::TextEdit;
use warble_engine::{Clip, MAX_SOUND_SECONDS, SoundId};

use crate::library::{self, Hit};
use crate::state::{State, Tab};
use crate::trim::{self, Trim};
use crate::youtube;

/// Where the YouTube link stands.
#[derive(Clone, Debug, PartialEq)]
pub enum YouTube {
    Idle,
    /// yt-dlp isn't here yet: say what will be downloaded before doing it.
    AskToInstall,
    Installing,
    Fetching,
    Failed(String),
}

/// Everything the Find sounds page and the trim editor keep. It always exists, so the text
/// fields always have somewhere to type into; `open` says whether the page shows.
pub struct Find {
    pub open: bool,
    pub query: TextEdit,
    pub link: TextEdit,
    pub key: TextEdit,
    /// The trim editor's name field.
    pub trim_name: TextEdit,
    pub searching: bool,
    /// The query the results are for, once a search has finished.
    pub searched: Option<String>,
    pub hits: Vec<Hit>,
    /// A site that didn't answer, or why nothing came back.
    pub problem: Option<String>,
    /// Results downloading (by url), and results added.
    pub busy: Vec<String>,
    pub added: Vec<String>,
    /// The result playing in the headphones.
    pub previewing: Option<String>,
    pub youtube: YouTube,
    /// The last clip the trim editor saved, to say so.
    pub saved: Option<String>,
    search_id: u64,
}

impl Find {
    pub fn new(freesound_key: &str) -> Find {
        Find {
            open: false,
            query: TextEdit::new(""),
            link: TextEdit::new(""),
            key: TextEdit::new(freesound_key),
            trim_name: TextEdit::new(""),
            searching: false,
            searched: None,
            hits: Vec::new(),
            problem: None,
            busy: Vec::new(),
            added: Vec::new(),
            previewing: None,
            youtube: YouTube::Idle,
            saved: None,
            search_id: 0,
        }
    }

    pub fn is_busy(&self, url: &str) -> bool {
        self.busy.iter().any(|u| u == url)
    }

    pub fn is_added(&self, url: &str) -> bool {
        self.added.iter().any(|u| u == url)
    }
}

impl State {
    pub fn open_find(&mut self) {
        self.set_tab(Tab::Soundboard);
        self.find.open = true;
        self.find.saved = None;
        // YouTube changes often; let yt-dlp update itself in the background, once a day.
        if self.handle.is_some() && !self.headless {
            let dir = self.work_dir();
            std::thread::spawn(move || youtube::update_if_due(&dir));
        }
    }

    pub fn close_find(&mut self) {
        self.stop_preview();
        self.find.open = false;
    }

    pub fn stop_preview(&mut self) {
        self.engine.stop_preview();
        self.status.preview = None;
        self.find.previewing = None;
    }

    pub fn set_freesound_key(&mut self) {
        self.settings.freesound_key = self.find.key.text.trim().to_string();
        self.changed();
    }

    // --- Search ---

    pub fn search(&mut self) {
        let query = self.find.query.text.trim().to_string();
        if query.is_empty() {
            return;
        }
        let f = &mut self.find;
        f.searching = true;
        f.search_id += 1;
        let (id, key) = (f.search_id, self.settings.freesound_key.clone());
        let q = query.clone();
        self.off_thread(
            move || library::search(&q, &key),
            move |s, found| {
                let f = &mut s.find;
                if f.search_id != id {
                    return; // a newer search is on its way
                }
                f.searching = false;
                f.hits = found.hits;
                f.problem = found.problem;
                f.searched = Some(query);
            },
        );
    }

    /// Fetches a result's file (once) and does `work` with it off the UI thread, then `done`
    /// on it. A failure becomes the notice.
    fn with_hit<T: Send + 'static>(
        &mut self,
        url: &str,
        work: impl FnOnce(&Hit) -> Result<T, String> + Send + 'static,
        done: impl FnOnce(&mut State, &Hit, T) + Send + 'static,
    ) {
        let Some(hit) = self.find.hits.iter().find(|h| h.url == url).cloned() else {
            return;
        };
        if self.find.is_busy(url) {
            return;
        }
        self.find.busy.push(hit.url.clone());
        self.off_thread(
            move || (work(&hit), hit),
            |s, (result, hit)| {
                s.find.busy.retain(|u| *u != hit.url);
                match result {
                    Ok(value) => done(s, &hit, value),
                    Err(e) => s.notice = Some(e),
                }
            },
        );
    }

    /// Plays a result in the headphones only; pressed again while it plays, stops it.
    pub fn preview_hit(&mut self, url: &str) {
        if self.find.previewing.as_deref() == Some(url) && self.status.preview.is_some() {
            self.stop_preview();
            return;
        }
        self.with_hit(
            url,
            |hit| Clip::decode(&library::download(hit)?, MAX_SOUND_SECONDS),
            |s, hit, clip| {
                if s.find.open && s.trim.is_none() {
                    s.engine.preview(&clip, 0..clip.samples().len());
                    s.status.preview = Some(0);
                    s.find.previewing = Some(hit.url.clone());
                }
            },
        );
    }

    /// Copies a result into the sounds folder and onto the board.
    pub fn add_hit(&mut self, url: &str) {
        self.with_hit(url, library::download, |s, hit, path| {
            s.find.added.push(hit.url.clone());
            s.load_paths(vec![path], true);
        });
    }

    pub fn trim_hit(&mut self, url: &str) {
        self.with_hit(
            url,
            |hit| Clip::decode(&library::download(hit)?, youtube::MAX_SECONDS),
            |s, hit, clip| s.open_trim(clip, &hit.name),
        );
    }

    // --- YouTube ---

    pub fn get_audio(&mut self) {
        let dir = self.work_dir();
        let f = &mut self.find;
        if matches!(f.youtube, YouTube::Installing | YouTube::Fetching) {
            return;
        }
        let link = f.link.text.trim().to_string();
        if !youtube::looks_like_link(&link) {
            f.youtube = YouTube::Failed("Paste a link that starts with https://".into());
            return;
        }
        if !youtube::tool_path(&dir).is_file() {
            f.youtube = YouTube::AskToInstall;
            return;
        }
        f.youtube = YouTube::Fetching;
        self.off_thread(
            move || {
                let out = std::env::temp_dir().join("warble-youtube");
                let (file, title) = youtube::fetch(&dir, &link, &out)?;
                let clip = Clip::decode(&file, youtube::MAX_SECONDS);
                let _ = std::fs::remove_file(&file);
                let clip = clip.map_err(|_| "Couldn't read that video's audio.".to_string())?;
                Ok((clip, title))
            },
            |s, result: Result<(Clip, String), String>| match result {
                Ok((clip, title)) => {
                    s.find.youtube = YouTube::Idle;
                    s.find.link = TextEdit::new("");
                    s.open_trim(clip, &title);
                }
                Err(e) => s.find.youtube = YouTube::Failed(e),
            },
        );
    }

    /// Downloads yt-dlp (after the notice saying so), then gets the audio.
    pub fn install_youtube_tool(&mut self) {
        if self.find.youtube == YouTube::Installing {
            return;
        }
        self.find.youtube = YouTube::Installing;
        let dir = self.work_dir();
        self.off_thread(
            move || youtube::install(&dir),
            |s, result| match result {
                Ok(()) => {
                    s.find.youtube = YouTube::Idle;
                    s.get_audio();
                }
                Err(e) => s.find.youtube = YouTube::Failed(e),
            },
        );
    }

    pub fn cancel_youtube(&mut self) {
        self.find.youtube = YouTube::Idle;
    }

    // --- Trim ---

    pub fn open_trim(&mut self, clip: Clip, name: &str) {
        self.stop_preview();
        self.set_tab(Tab::Soundboard);
        self.find.trim_name = TextEdit::new(library::safe_name(name));
        self.trim = Some(Trim::new(clip));
    }

    pub fn close_trim(&mut self) {
        self.stop_preview();
        self.trim = None;
    }

    /// The trim editor for a tile's file. Saving makes a new tile; this one stays.
    pub fn trim_sound(&mut self, id: SoundId) {
        let Some(sound) = self.sounds.iter().find(|s| s.id == id) else {
            return;
        };
        let (path, name) = (sound.path.clone(), format!("{} (trimmed)", sound.name));
        self.off_thread(
            move || Clip::decode(&path, youtube::MAX_SECONDS),
            move |s, clip| match clip {
                Ok(clip) => s.open_trim(clip, &name),
                Err(e) => s.notice = Some(e),
            },
        );
    }

    /// Plays the selection in the headphones only, or stops it.
    pub fn trim_play(&mut self) {
        if self.status.preview.is_some() {
            self.stop_preview();
        } else if let Some(t) = &self.trim {
            let range = t.range();
            self.status.preview = Some(range.start);
            self.engine.preview(&t.clip, range);
        }
    }

    /// Nudges a handle (0 the start, 1 the end) by arrow-key steps.
    pub fn step_trim(&mut self, end: bool, steps: i32) {
        if let Some(t) = &mut self.trim {
            let by = steps as f64 * t.step();
            if end {
                t.set_end(t.end + by);
            } else {
                t.set_start(t.start + by);
            }
        }
    }

    /// Writes the selection as a WAV into the sounds folder and adds its tile.
    pub fn trim_save(&mut self) {
        let dir = self
            .sounds_dir()
            .unwrap_or_else(|| self.work_dir().join("sounds"));
        let name = trim::file_name(&self.find.trim_name.text);
        let Some(t) = &mut self.trim else { return };
        if t.saving {
            return;
        }
        t.saving = true;
        let (clip, range) = (t.clip.clone(), t.range());
        self.off_thread(
            move || trim::save(&dir, &name, &clip, range),
            |s, saved| match saved {
                Ok(path) => {
                    s.close_trim();
                    s.find.saved = Some(crate::settings::sound_name(&path));
                    s.load_paths(vec![path], true);
                }
                Err(e) => {
                    if let Some(t) = &mut s.trim {
                        t.saving = false;
                    }
                    s.notice = Some(format!("Couldn't save the clip: {e}"));
                }
            },
        );
    }
}
