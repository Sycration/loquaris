//! Validating clips and combining a lesson's recordings into a 48 kHz,
//! stereo, 16-bit WAV.
use std::{io::Cursor, ops::Range};

use loquaris_core::{AudioGaps, Participant, Project, Trim};

use crate::ClipLoader;

const RATE: u32 = 48_000;
/// Uploads larger than this are refused.
pub const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub include_teacher: bool,
    pub include_student: bool,
}

/// Properties of a validated WAV file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub frames: u32,
    pub channels: u16,
}

/// Fully decode a WAV file to check it, returning its format.
pub fn validate(bytes: &[u8]) -> Result<WavInfo, String> {
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err("WAV file exceeds the 100 MiB import limit.".into());
    }
    inspect(bytes)
}

/// [`validate`] without the size limit, for clips that already exist, such
/// as the desktop app's recordings of up to ten minutes.
pub(crate) fn inspect(bytes: &[u8]) -> Result<WavInfo, String> {
    let (spec, frames) = read_samples(bytes, |_| ()).map_err(|e| format!("Invalid WAV: {e}"))?;
    Ok(WavInfo {
        sample_rate: spec.sample_rate,
        frames,
        channels: spec.channels,
    })
}

/// Decode every sample as `f32` in -1..1, interleaved, passing each to
/// `sample`. Returns the format and the number of frames; an empty clip, a
/// partial frame or a non-finite sample is an error.
fn read_samples(
    bytes: &[u8],
    mut sample: impl FnMut(f32),
) -> Result<(hound::WavSpec, u32), String> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels == 0
        || spec.sample_rate == 0
        || spec.bits_per_sample == 0
        || spec.bits_per_sample > 32
    {
        return Err("invalid WAV format".into());
    }
    let mut count = 0u64;
    let mut each = |value: f32| {
        if !value.is_finite() {
            return Err("the recording contains invalid samples".to_owned());
        }
        count += 1;
        sample(value);
        Ok(())
    };
    if spec.sample_format == hound::SampleFormat::Float {
        for value in reader.samples::<f32>() {
            each(value.map_err(|e| e.to_string())?)?;
        }
    } else {
        let scale = (2f64).powi(i32::from(spec.bits_per_sample) - 1) as f32;
        for value in reader.samples::<i32>() {
            each(value.map_err(|e| e.to_string())? as f32 / scale)?;
        }
    }
    let channels = u64::from(spec.channels);
    if count == 0 {
        return Err("the recording is empty".into());
    }
    if !count.is_multiple_of(channels) {
        return Err("the recording ends with a partial frame".into());
    }
    let frames = u32::try_from(count / channels).map_err(|_| "the recording is too long")?;
    Ok((spec, frames))
}

struct Clip {
    rate: u32,
    channels: usize,
    samples: Vec<f32>,
}

fn decode(bytes: &[u8]) -> Result<Clip, String> {
    let mut samples = Vec::new();
    let (spec, _) = read_samples(bytes, |value| samples.push(value))?;
    Ok(Clip {
        rate: spec.sample_rate,
        channels: usize::from(spec.channels),
        samples,
    })
}

/// The frames of a clip inside a crop. A crop that does not fit the clip is
/// clamped to it. A non-empty clip always has at least one selected frame.
pub(crate) fn selected_frames(trim: Option<Trim>, total: u32) -> Range<u32> {
    let Some(trim) = trim else {
        return 0..total;
    };
    let start = trim.start.min(total.saturating_sub(1));
    start..trim.end.max(start.saturating_add(1)).min(total)
}

/// Gain at `frame` that keeps a crop from clicking: a 10 ms ramp on each edge
/// that cuts into the clip. Edges at the clip's own ends are left untouched.
fn cut_gain(frame: u32, selected: &Range<u32>, total: u32, rate: u32) -> f32 {
    let fade = (rate / 100).max(1);
    let mut distance = fade;
    if selected.start > 0 {
        distance = distance.min(frame.saturating_sub(selected.start));
    }
    if selected.end < total {
        distance = distance.min(selected.end.saturating_sub(frame + 1));
    }
    distance as f32 / fade as f32
}

fn validate_gaps(gaps: AudioGaps) -> Result<(), String> {
    if gaps.same_participant_ms > AudioGaps::MAX_MS
        || gaps.different_participant_ms > AudioGaps::MAX_MS
    {
        return Err("Audio gaps must be between 0 and 60 seconds.".into());
    }
    Ok(())
}

/// The lesson's audio as one 48 kHz stereo 16-bit WAV file: every clip's
/// crop, resampled by linear interpolation, with the project's gaps between
/// clips and none before the first. An excluded participant's clips become
/// silence of the same length, so the timing does not change.
pub fn render_lesson(
    project: &Project,
    lesson_id: &str,
    options: Options,
    load: &mut ClipLoader,
) -> Result<Vec<u8>, String> {
    let gaps = project.gaps;
    validate_gaps(gaps)?;
    let lesson = project
        .lessons
        .iter()
        .find(|lesson| lesson.id == lesson_id)
        .ok_or("The selected lesson no longer exists.")?;
    if lesson.entries.is_empty() || lesson.entries.iter().any(|entry| entry.audio.is_none()) {
        return Err(format!(
            "Lesson ‘{}’ is not fully recorded. All selected lessons must be fully recorded.",
            lesson.display_name()
        ));
    }
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut output = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut output, spec).map_err(|e| e.to_string())?;
    let mut previous = None;
    for (index, entry) in lesson.entries.iter().enumerate() {
        let result = (|| -> Result<(), String> {
            let audio = entry.audio.as_ref().ok_or("Missing recording")?;
            let clip = decode(&load(audio)?)?;
            if let Some(participant) = previous {
                let frames = u64::from(gaps.between(participant, entry.participant))
                    * u64::from(RATE)
                    / 1000;
                write_silence(&mut writer, frames * 2)?;
            }
            previous = Some(entry.participant);
            let enabled = match entry.participant {
                Participant::Teacher => options.include_teacher,
                Participant::Student => options.include_student,
            };
            write_clip(&mut writer, &clip, entry.trim, enabled)
        })();
        result.map_err(|e| {
            format!(
                "Lesson ‘{}’, entry {}: {e}",
                lesson.display_name(),
                index + 1
            )
        })?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(output.into_inner())
}

type Writer<'a> = hound::WavWriter<&'a mut Cursor<Vec<u8>>>;

/// hound's sample writer takes a sample count up front; write in blocks so
/// the count always fits.
const BLOCK: u64 = 1 << 20;

fn write_silence(writer: &mut Writer, samples: u64) -> Result<(), String> {
    let mut left = samples;
    while left > 0 {
        let count = left.min(BLOCK);
        let mut block = writer.get_i16_writer(count as u32);
        for _ in 0..count {
            block.write_sample(0);
        }
        block.flush().map_err(|e| e.to_string())?;
        left -= count;
    }
    Ok(())
}

/// Resample the clip's crop to 48 kHz stereo. Mono is copied to both
/// channels; with more than two, even channels are averaged to the left and
/// odd ones to the right.
fn write_clip(
    writer: &mut Writer,
    clip: &Clip,
    trim: Option<Trim>,
    enabled: bool,
) -> Result<(), String> {
    let channels = clip.channels;
    let total = (clip.samples.len() / channels) as u32;
    let selected = selected_frames(trim, total);
    let first = selected.start as usize;
    let samples = &clip.samples[first * channels..selected.end as usize * channels];
    let frames = samples.len() / channels;
    let output_frames =
        (frames as u64 * u64::from(RATE) + u64::from(clip.rate) / 2) / u64::from(clip.rate);
    let mix = |frame: usize, channel: usize| {
        if channels <= 2 {
            samples[frame * channels + if channels == 1 { 0 } else { channel }]
        } else {
            let values = &samples[frame * channels..(frame + 1) * channels];
            let mut count = 0;
            let sum: f32 = values
                .iter()
                .skip(channel)
                .step_by(2)
                .map(|value| {
                    count += 1;
                    *value
                })
                .sum();
            sum / count as f32
        }
    };
    let mut frame = 0;
    while frame < output_frames {
        let end = (frame + BLOCK / 2).min(output_frames);
        let mut block = writer.get_i16_writer(((end - frame) * 2) as u32);
        for frame in frame..end {
            let pos = frame as f64 * f64::from(clip.rate) / f64::from(RATE);
            let a = (pos as usize).min(frames - 1);
            let b = (a + 1).min(frames - 1);
            let fraction = (pos - a as f64) as f32;
            let gain = cut_gain((first + a) as u32, &selected, total, clip.rate);
            for channel in 0..2 {
                let value = if enabled {
                    (mix(a, channel) * (1.0 - fraction) + mix(b, channel) * fraction) * gain
                } else {
                    0.0
                };
                block.write_sample((value.clamp(-1.0, 1.0) * 32767.0).round() as i16);
            }
        }
        block.flush().map_err(|e| e.to_string())?;
        frame = end;
    }
    Ok(())
}

/// The file name for each lesson's WAV export, in lesson order; an error
/// if two would collide.
pub fn file_names(project: &Project) -> Result<Vec<String>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::with_capacity(project.lessons.len());
    for lesson in &project.lessons {
        let name = format!(
            "{} - {}.wav",
            portable_stem(&project.name),
            portable_stem(&lesson.name)
        );
        if !seen.insert(name.to_lowercase()) {
            return Err(format!(
                "Lesson filenames conflict: {name}. Rename a lesson before exporting."
            ));
        }
        names.push(name);
    }
    Ok(names)
}

/// A portable file name stem: characters Windows forbids become `_`, ends
/// lose spaces and dots, an empty name becomes "project" and reserved device
/// names get a leading `_`. A name that itself ends in `.loqr` keeps it, as
/// in the desktop app.
fn portable_stem(name: &str) -> String {
    let suggested = suggested_file_name(name);
    if name
        .trim_matches(|c| c == ' ' || c == '.')
        .to_ascii_lowercase()
        .ends_with(".loqr")
    {
        return suggested;
    }
    suggested
        .strip_suffix(".loqr")
        .or_else(|| suggested.strip_suffix(".LOQR"))
        .unwrap_or(&suggested)
        .to_owned()
}

/// The desktop app's suggested `.loqr` file name for a project title.
pub(crate) fn suggested_file_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| {
            if character.is_control() || "<>:\"/\\|?*".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches(|character| character == ' ' || character == '.');
    let mut stem = if sanitized.is_empty() {
        "project".to_owned()
    } else {
        sanitized.to_owned()
    };
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        stem.insert(0, '_');
    }
    if !stem.to_ascii_lowercase().ends_with(".loqr") {
        stem.push_str(".loqr");
    }
    stem
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use loquaris_core::{AudioRef, Language};

    use super::*;
    use crate::tests::{entry, lesson, project, wav};

    /// Clips by blob, and the loader that serves them.
    #[derive(Default)]
    struct Store(HashMap<String, Vec<u8>>);

    impl Store {
        fn add(&mut self, bytes: Vec<u8>) -> AudioRef {
            let info = validate(&bytes).unwrap();
            let blob = format!("clip{}", self.0.len());
            self.0.insert(blob.clone(), bytes);
            AudioRef {
                blob,
                sample_rate: info.sample_rate,
                frames: info.frames,
                channels: info.channels,
            }
        }

        fn render(
            &self,
            project: &Project,
            index: usize,
            options: Options,
        ) -> Result<Vec<u8>, String> {
            let mut load = |audio: &AudioRef| {
                self.0
                    .get(&audio.blob)
                    .cloned()
                    .ok_or_else(|| "missing clip".to_owned())
            };
            render_lesson(project, &project.lessons[index].id, options, &mut load)
        }
    }

    const ALL: Options = Options {
        include_teacher: true,
        include_student: true,
    };

    /// One second of a constant value.
    fn constant(rate: u32, channels: u16, value: i16) -> Vec<u8> {
        wav(rate, channels, rate, |_, _| value)
    }

    fn samples(bytes: &[u8]) -> Vec<i16> {
        let reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        assert_eq!(reader.spec().sample_rate, RATE);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().bits_per_sample, 16);
        reader.into_samples::<i16>().map(Result::unwrap).collect()
    }

    /// The legacy fixture: lesson "One" has a teacher clip at 8 kHz mono and a
    /// student clip at 16 kHz stereo; lesson "Two" one teacher clip at 48 kHz.
    fn fixture() -> (Project, Store) {
        let mut store = Store::default();
        let mut project = project(vec![
            lesson(
                "One",
                vec![
                    entry(Participant::Teacher, Language::L1, ""),
                    entry(Participant::Student, Language::Latin, ""),
                ],
            ),
            lesson(
                "Two",
                vec![entry(Participant::Teacher, Language::Latin, "")],
            ),
        ]);
        project.lessons[0].entries[0].audio = Some(store.add(constant(8000, 1, 12000)));
        project.lessons[0].entries[1].audio = Some(store.add(constant(16000, 2, -12000)));
        project.lessons[1].entries[0].audio = Some(store.add(constant(48000, 1, 6000)));
        (project, store)
    }

    #[test]
    fn validate_accepts_any_format_and_rejects_garbage() {
        let info = validate(&constant(22050, 3, 5)).unwrap();
        assert_eq!(
            info,
            WavInfo {
                sample_rate: 22050,
                frames: 22050,
                channels: 3
            }
        );
        for (bits, format) in [
            (8, hound::SampleFormat::Int),
            (24, hound::SampleFormat::Int),
            (32, hound::SampleFormat::Int),
            (32, hound::SampleFormat::Float),
        ] {
            let mut data = Cursor::new(Vec::new());
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 44100,
                bits_per_sample: bits,
                sample_format: format,
            };
            let mut writer = hound::WavWriter::new(&mut data, spec).unwrap();
            for _ in 0..10 {
                if format == hound::SampleFormat::Float {
                    writer.write_sample(0.25f32).unwrap();
                } else {
                    writer.write_sample(1i32).unwrap();
                }
            }
            writer.finalize().unwrap();
            assert_eq!(
                validate(data.get_ref()).unwrap().frames,
                10,
                "{bits} {format:?}"
            );
        }
        assert!(validate(b"").is_err());
        assert!(validate(b"RIFF garbage").is_err());
        assert!(
            validate(&wav(8000, 1, 0, |_, _| 0))
                .unwrap_err()
                .contains("empty")
        );
        let mut truncated = constant(8000, 2, 1);
        truncated.truncate(truncated.len() - 3);
        assert!(validate(&truncated).is_err());
        assert!(
            validate(&vec![0; MAX_UPLOAD_BYTES + 1])
                .unwrap_err()
                .contains("100 MiB")
        );
        // Not finite.
        let mut data = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::new(&mut data, spec).unwrap();
        writer.write_sample(f32::NAN).unwrap();
        writer.finalize().unwrap();
        assert!(validate(data.get_ref()).is_err());
    }

    #[test]
    fn selected_frames_fit_any_clip() {
        let trim = |start, end| Some(Trim { start, end });
        assert_eq!(selected_frames(None, 10), 0..10);
        assert_eq!(selected_frames(trim(2, 6), 10), 2..6);
        assert_eq!(selected_frames(trim(2, 60), 10), 2..10);
        assert_eq!(selected_frames(trim(20, 60), 10), 9..10);
        assert_eq!(selected_frames(trim(4, 4), 10), 4..5);
        assert_eq!(selected_frames(trim(4, 8), 0), 0..0);
    }

    #[test]
    fn cut_gain_ramps_only_edges_that_cut_into_the_clip() {
        let selected = 1000..3000;
        assert_eq!(cut_gain(1000, &selected, 4000, 10_000), 0.0);
        assert_eq!(cut_gain(1050, &selected, 4000, 10_000), 0.5);
        assert_eq!(cut_gain(2000, &selected, 4000, 10_000), 1.0);
        assert_eq!(cut_gain(2999, &selected, 4000, 10_000), 0.0);
        assert_eq!(cut_gain(0, &(0..4000), 4000, 10_000), 1.0);
        assert_eq!(cut_gain(3999, &(0..4000), 4000, 10_000), 1.0);
    }

    #[test]
    fn combines_clips_and_mutes_without_changing_timing() {
        let (project, store) = fixture();
        for (teacher, student) in [(true, true), (true, false), (false, true), (false, false)] {
            let options = Options {
                include_teacher: teacher,
                include_student: student,
            };
            let samples = samples(&store.render(&project, 0, options).unwrap());
            // 1 s, 0.5 s gap, 1 s, stereo.
            assert_eq!(samples.len(), 5 * RATE as usize);
            assert_eq!(samples[0] != 0, teacher);
            assert!(samples[96_000..144_000].iter().all(|&s| s == 0));
            assert_eq!(samples[144_000] != 0, student);
        }
        let second = store.render(&project, 1, ALL).unwrap();
        assert_eq!(
            hound::WavReader::new(Cursor::new(&second))
                .unwrap()
                .duration(),
            RATE
        );
    }

    #[test]
    fn resamples_by_linear_interpolation_and_rounds() {
        // A 1 kHz ramp 0, 100, 200, … resampled to 48 kHz: each output frame
        // interpolates two input frames 1/48 apart.
        let mut store = Store::default();
        let mut project = project(vec![lesson(
            "One",
            vec![entry(Participant::Teacher, Language::Latin, "")],
        )]);
        project.lessons[0].entries[0].audio =
            Some(store.add(wav(1000, 1, 10, |frame, _| frame as i16 * 100)));
        let samples = samples(&store.render(&project, 0, ALL).unwrap());
        assert_eq!(samples.len(), 480 * 2);
        let expected = |frame: usize| {
            let pos = frame as f64 / 48.0;
            let a = (pos as usize).min(9);
            let b = (a + 1).min(9);
            let fraction = pos - a as f64;
            let value =
                (a as f64 * 100.0 * (1.0 - fraction) + b as f64 * 100.0 * fraction) / 32768.0;
            (value * 32767.0).round() as i16
        };
        for frame in [0, 1, 24, 47, 48, 100, 431, 432, 479] {
            assert!(
                (samples[frame * 2] - expected(frame)).abs() <= 1,
                "frame {frame}: {} vs {}",
                samples[frame * 2],
                expected(frame)
            );
            assert_eq!(samples[frame * 2], samples[frame * 2 + 1]);
        }
        // The last input frame is held rather than read past the end.
        assert_eq!(samples[479 * 2], expected(479));
        // 44.1 kHz → 48 kHz rounds the frame count.
        project.lessons[0].entries[0].audio = Some(store.add(wav(44_100, 1, 441, |_, _| 0)));
        assert_eq!(samples_len(&store.render(&project, 0, ALL).unwrap()), 480);
    }

    fn samples_len(bytes: &[u8]) -> u32 {
        hound::WavReader::new(Cursor::new(bytes))
            .unwrap()
            .duration()
    }

    #[test]
    fn maps_channels_to_stereo() {
        let mut store = Store::default();
        let mut project = project(vec![lesson(
            "One",
            vec![
                entry(Participant::Teacher, Language::Latin, ""),
                entry(Participant::Teacher, Language::Latin, ""),
                entry(Participant::Teacher, Language::Latin, ""),
            ],
        )]);
        project.gaps.same_participant_ms = 0;
        let entries = &mut project.lessons[0].entries;
        entries[0].audio = Some(store.add(wav(48_000, 1, 10, |_, _| 1000)));
        entries[1].audio = Some(store.add(wav(48_000, 2, 10, |_, channel| {
            [2000, -2000][usize::from(channel)]
        })));
        // Left = mean(100, 300, 500), right = mean(200, 400).
        entries[2].audio = Some(store.add(wav(48_000, 5, 10, |_, channel| {
            [100, 200, 300, 400, 500][usize::from(channel)]
        })));
        let samples = samples(&store.render(&project, 0, ALL).unwrap());
        let scaled = |value: f32| ((value / 32768.0) * 32767.0).round() as i16;
        assert_eq!(&samples[0..2], [scaled(1000.0), scaled(1000.0)]);
        assert_eq!(&samples[20..22], [scaled(2000.0), scaled(-2000.0)]);
        assert_eq!(&samples[40..42], [scaled(300.0), scaled(300.0)]);
        assert_eq!(samples.len(), 60);
    }

    #[test]
    fn exports_only_the_selected_part_with_faded_cuts() {
        // One second at 48 kHz, cropped to the half second starting at 0.25 s.
        let (mut project, store) = fixture();
        project.lessons[1].entries[0].trim = Some(Trim {
            start: 12_000,
            end: 36_000,
        });
        let bytes = store.render(&project, 1, ALL).unwrap();
        assert_eq!(samples_len(&bytes), 24_000);
        let cropped = samples(&bytes);
        assert_eq!(cropped[0], 0);
        // Frame 240 is halfway through the 10 ms ramp.
        assert!((2990..=3000).contains(&cropped[480]), "{}", cropped[480]);
        assert!((5990..=6000).contains(&cropped[24_000]));
        assert!(cropped[cropped.len() - 1].abs() < 20);
        // A crop that keeps the clip's own start ramps only the cut end.
        project.lessons[1].entries[0].trim = Some(Trim {
            start: 0,
            end: 24_000,
        });
        let samples = samples(&store.render(&project, 1, ALL).unwrap());
        assert!((5990..=6000).contains(&samples[0]));
        assert_eq!(samples[samples.len() - 1], 0);
    }

    #[test]
    fn incomplete_or_damaged_audio_is_an_error() {
        let (mut project, mut store) = fixture();
        project.lessons[0].entries[1].audio = None;
        let error = store.render(&project, 0, ALL).unwrap_err();
        assert_eq!(
            error,
            "Lesson ‘One’ is not fully recorded. All selected lessons must be fully recorded."
        );
        project.lessons[0].entries[1].audio = Some(AudioRef {
            blob: "invalid".into(),
            sample_rate: 8000,
            frames: 1,
            channels: 1,
        });
        store.0.insert("invalid".into(), b"invalid".to_vec());
        let error = store.render(&project, 0, ALL).unwrap_err();
        assert!(error.starts_with("Lesson ‘One’, entry 2: "), "{error}");
        project.lessons[0].entries[1].audio.as_mut().unwrap().blob = "gone".into();
        assert!(
            store
                .render(&project, 0, ALL)
                .unwrap_err()
                .ends_with("missing clip")
        );
        project.lessons[0].entries.clear();
        assert!(
            store
                .render(&project, 0, ALL)
                .unwrap_err()
                .contains("fully recorded")
        );
        let mut load = |_: &AudioRef| Ok(Vec::new());
        assert!(render_lesson(&project, "nope", ALL, &mut load).is_err());
        project.gaps.same_participant_ms = 60_001;
        assert!(store.render(&project, 1, ALL).unwrap_err().contains("gaps"));
    }

    #[test]
    fn file_names_are_portable_and_unique() {
        let (mut project, _) = fixture();
        assert_eq!(
            file_names(&project).unwrap(),
            ["Course - One.wav", "Course - Two.wav"]
        );
        project.name = "Latin/2026".into();
        project.lessons[0].name = "CON".into();
        project.lessons[1].name = String::new();
        assert_eq!(
            file_names(&project).unwrap(),
            ["Latin_2026 - _CON.wav", "Latin_2026 - project.wav"]
        );
        project.name = "Course.loqr".into();
        project.lessons[0].name = " Ūna. ".into();
        assert_eq!(file_names(&project).unwrap()[0], "Course.loqr - Ūna.wav");
        project.lessons[1].name = "ŪNA".into();
        let error = file_names(&project).unwrap_err();
        assert!(error.contains("conflict"), "{error}");
    }

    #[test]
    fn suggested_name_uses_project_title_and_is_portable() {
        assert_eq!(suggested_file_name("Via nātūrālis"), "Via nātūrālis.loqr");
        assert_eq!(suggested_file_name("Latin/2026"), "Latin_2026.loqr");
        assert_eq!(suggested_file_name("CON"), "_CON.loqr");
        assert_eq!(suggested_file_name("Course.LOQR"), "Course.LOQR");
        assert_eq!(suggested_file_name("  "), "project.loqr");
    }

    #[test]
    fn participant_gaps_use_settings_even_when_muted() {
        let (mut project, store) = fixture();
        let third = project.lessons[1].entries[0].clone();
        let entries = &mut project.lessons[0].entries;
        entries[1].participant = Participant::Teacher;
        entries.push(third);
        entries[2].participant = Participant::Student;
        let teacher_only = Options {
            include_teacher: true,
            include_student: false,
        };
        for (gaps, same_frames, different_frames) in [
            (AudioGaps::default(), 4800, 24000),
            (
                AudioGaps {
                    same_participant_ms: 250,
                    different_participant_ms: 750,
                },
                12000,
                36000,
            ),
            (
                AudioGaps {
                    same_participant_ms: 0,
                    different_participant_ms: 0,
                },
                0,
                0,
            ),
        ] {
            project.gaps = gaps;
            let bytes = store.render(&project, 0, teacher_only).unwrap();
            assert_eq!(
                samples_len(&bytes),
                RATE * 3 + same_frames + different_frames
            );
            let samples = samples(&bytes);
            let at = |frame: u32| samples[frame as usize * 2];
            if same_frames > 0 {
                assert_eq!(at(RATE), 0);
            }
            assert_ne!(at(RATE + same_frames), 0);
            if different_frames > 0 {
                assert_eq!(at(RATE * 2 + same_frames), 0);
            }
            assert_eq!(at(RATE * 2 + same_frames + different_frames), 0);
        }
    }
}
