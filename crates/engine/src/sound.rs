//! Soundboard clip decoding (symphonia): any WAV, MP3, OGG/Vorbis, FLAC or M4A (AAC, including
//! HE-AAC's core) file to mono `f32` at the file's own sample rate. Runs on the caller's thread.

use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::graph::Sound;

/// Longest clip we accept. A 5-minute clip at 48 kHz is about 55 MB in memory.
pub(crate) const MAX_SECONDS: u64 = 5 * 60;

pub(crate) fn decode(path: &Path) -> Result<Sound, String> {
    decode_capped(path, MAX_SECONDS)
}

/// `decode`, refusing clips longer than `max_seconds` (a whole number of minutes). Each error
/// names the file and says what to do next.
pub(crate) fn decode_capped(path: &Path, max_seconds: u64) -> Result<Sound, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let unsupported = || {
        format!(
            "Warble can’t play “{name}”. It may be damaged, or renamed from another kind of file. Try another copy."
        )
    };
    let no_audio = || format!("There’s no sound in “{name}”. Try another file.");
    // The file is fine, but its kind of audio (Opus, ALAC) isn't one Warble decodes.
    let codec = || {
        format!(
            "Warble can’t play the kind of audio in “{name}”. Save it as MP3 or WAV, then add it again."
        )
    };

    let file = File::open(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => {
            format!("Couldn’t find “{name}”. If you moved it, add it again from its new folder.")
        }
        ErrorKind::PermissionDenied => format!(
            "Warble isn’t allowed to open “{name}”. Copy it to your Music folder and add it from there."
        ),
        _ => format!("Couldn’t open “{name}”. Close any app that’s using it, then try again."),
    })?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| unsupported())?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(no_audio)?;
    let track_id = track.id;
    let mut params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(codec)?
        .clone();
    if params.codec == CODEC_ID_AAC
        && let Some(core) = params.extra_data.as_deref().and_then(aac_lc_core)
    {
        params.extra_data = Some(core);
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|_| codec())?;

    let mut samples: Vec<f32> = Vec::new();
    let mut rate = 0;
    let mut planes: Vec<Vec<f32>> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(Error::ResetRequired) => break,
            Err(Error::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
            // A damaged tail: keep what decoded.
            Err(_) if !samples.is_empty() => break,
            Err(_) => return Err(format!("“{name}” is damaged. Try another copy.")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(Error::DecodeError(_) | Error::IoError(_)) => continue,
            Err(_) => return Err(codec()),
        };
        if rate == 0 {
            rate = audio.spec().rate();
        }
        // Downmix to mono by averaging the channels.
        audio.copy_to_vecs_planar::<f32>(&mut planes);
        let start = samples.len();
        samples.resize(start + audio.frames(), 0.0);
        let scale = 1.0 / planes.len().max(1) as f32;
        for plane in &planes {
            for (out, s) in samples[start..].iter_mut().zip(plane) {
                *out += s * scale;
            }
        }
        if samples.len() as u64 > max_seconds * rate as u64 {
            return Err(format!(
                "“{name}” is longer than {} minutes. Pick a shorter sound.",
                max_seconds / 60
            ));
        }
    }
    if samples.is_empty() || rate == 0 {
        return Err(no_audio());
    }
    samples.shrink_to_fit();
    Ok(Sound { samples, rate })
}

/// HE-AAC (v1 adds SBR, v2 parametric stereo too) is plain AAC-LC at half the sample rate, plus
/// side data that rebuilds the top octave. Symphonia decodes only AAC-LC, so for an HE-AAC
/// AudioSpecificConfig this returns one for its LC core: the clip plays at the core's rate,
/// without the top octave (above about 11 kHz), and the side data is skipped. Instagram reels
/// are HE-AAC. `None` for anything else.
fn aac_lc_core(asc: &[u8]) -> Option<Box<[u8]>> {
    let mut at = 0;
    let mut read = |n: usize| -> Option<u32> {
        let mut v = 0;
        for _ in 0..n {
            let bit = asc.get(at / 8)? >> (7 - at % 8) & 1;
            v = v << 1 | u32::from(bit);
            at += 1;
        }
        Some(v)
    };
    // Explicit HE-AAC signalling: object type 5 (SBR) or 29 (PS), then the core's sample rate
    // index and channels, the full rate's index, and the core's object type, which must be LC.
    let object_type = read(5)?;
    if object_type != 5 && object_type != 29 {
        return None;
    }
    let core_rate = read(4)?;
    let channels = read(4)?;
    // 15 escapes to a 24-bit rate: not a core rate this rebuilds.
    if core_rate == 15 || read(4)? == 15 || read(5)? != 2 {
        return None;
    }
    // Object type 2 (LC), the core's rate and channels, then GASpecificConfig: 1024-sample
    // frames, no core coder, no extension.
    let lc = (2 << 11) | (core_rate << 7) | (channels << 3);
    Some(Box::new((lc as u16).to_be_bytes()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn he_aac_hands_over_its_lc_core() {
        // HE-AAC, 44.1 kHz stereo (an Instagram reel's): its core is LC at 22.05 kHz stereo.
        assert_eq!(
            aac_lc_core(&[0x2b, 0x92, 0x08, 0x00]).as_deref(),
            Some(&[0x13, 0x90][..])
        );
        // HE-AAC v2 at 48 kHz: a mono LC core at 24 kHz.
        assert_eq!(
            aac_lc_core(&[0xeb, 0x09, 0x88, 0x00]).as_deref(),
            Some(&[0x13, 0x08][..])
        );
        // Plain LC (YouTube's) and anything short or odd is left alone.
        assert_eq!(aac_lc_core(&[0x12, 0x10]), None);
        assert_eq!(aac_lc_core(&[0x2b]), None);
        assert_eq!(aac_lc_core(&[]), None);
    }

    /// A 16-bit PCM WAV file from interleaved samples, written by hand.
    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    pub(crate) fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("warble-engine-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub(crate) fn small_wav() -> Vec<u8> {
        // 0.1 s of stereo at 22050 Hz: left +0.5, right -0.25.
        wav(22050, 2, &[16384, -8192].repeat(2205))
    }

    #[test]
    fn decodes_and_downmixes_a_wav() {
        let path = temp_file("stereo.wav", &small_wav());
        let sound = decode(&path).expect("decodes");
        assert_eq!(sound.rate, 22050);
        assert_eq!(sound.samples.len(), 2205);
        assert!(
            sound.samples.iter().all(|s| (s - 0.125).abs() < 1e-3),
            "averaged to mono"
        );
    }

    #[test]
    fn garbage_gets_a_plain_error() {
        let path = temp_file("noise.mp3", &[0x5a; 4096]);
        let err = decode(&path).err().expect("rejected");
        assert_eq!(
            err,
            "Warble can’t play “noise.mp3”. It may be damaged, or renamed from another kind of \
             file. Try another copy."
        );
        let err = decode(Path::new("/definitely/not/here.wav"))
            .err()
            .expect("rejected");
        assert_eq!(
            err,
            "Couldn’t find “here.wav”. If you moved it, add it again from its new folder."
        );
        let err = decode(&temp_file("empty.wav", &wav(8000, 1, &[])))
            .err()
            .expect("rejected");
        assert_eq!(err, "There’s no sound in “empty.wav”. Try another file.");
    }

    #[test]
    fn rejects_clips_over_five_minutes() {
        let samples = vec![0i16; 8000 * (MAX_SECONDS as usize + 1)];
        let path = temp_file("long.wav", &wav(8000, 1, &samples));
        let err = decode(&path).err().expect("rejected");
        assert_eq!(
            err,
            "“long.wav” is longer than 5 minutes. Pick a shorter sound."
        );
    }
}
