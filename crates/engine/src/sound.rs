//! Soundboard clip decoding (symphonia): any WAV, MP3, OGG/Vorbis or FLAC file to mono `f32`
//! at the file's own sample rate. Runs on the caller's thread.

use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::graph::Sound;

/// Longest clip we accept. A 5-minute clip at 48 kHz is about 55 MB in memory.
pub(crate) const MAX_SECONDS: u64 = 5 * 60;

pub(crate) fn decode(path: &Path) -> Result<Sound, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let fail = |why: &str| format!("Couldn't read {name}: {why}");

    let file = File::open(path).map_err(|e| {
        fail(match e.kind() {
            ErrorKind::NotFound => "file not found",
            ErrorKind::PermissionDenied => "permission denied",
            _ => "couldn't open the file",
        })
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
        .map_err(|_| fail("unsupported format"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| fail("there's no audio in it"))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| fail("unsupported format"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| fail("unsupported format"))?;

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
            Err(_) => return Err(fail("the file is damaged")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(Error::DecodeError(_) | Error::IoError(_)) => continue,
            Err(_) => return Err(fail("unsupported format")),
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
        if samples.len() as u64 > MAX_SECONDS * rate as u64 {
            return Err(format!(
                "{name} is longer than 5 minutes. Pick a shorter clip."
            ));
        }
    }
    if samples.is_empty() || rate == 0 {
        return Err(fail("there's no audio in it"));
    }
    samples.shrink_to_fit();
    Ok(Sound { samples, rate })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

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
        assert_eq!(err, "Couldn't read noise.mp3: unsupported format");
        let err = decode(Path::new("/definitely/not/here.wav"))
            .err()
            .expect("rejected");
        assert_eq!(err, "Couldn't read here.wav: file not found");
    }

    #[test]
    fn rejects_clips_over_five_minutes() {
        let samples = vec![0i16; 8000 * (MAX_SECONDS as usize + 1)];
        let path = temp_file("long.wav", &wav(8000, 1, &samples));
        let err = decode(&path).err().expect("rejected");
        assert_eq!(
            err,
            "long.wav is longer than 5 minutes. Pick a shorter clip."
        );
    }
}
