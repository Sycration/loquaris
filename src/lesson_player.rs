//! Sequential lesson playback, including the project's inter-entry gaps.
use std::{
    io::Cursor,
    num::{NonZeroU16, NonZeroU32},
};

use crate::{Lesson, project::AudioGaps};
use base64::{Engine, engine::general_purpose::STANDARD};

struct Plan {
    clips: Vec<rodio::Decoder<Cursor<Vec<u8>>>>,
    gaps_ms: Vec<u32>,
    missing: Option<usize>,
}

impl Plan {
    fn new(lesson: &Lesson, start: usize, gaps: AudioGaps) -> Result<Self, String> {
        gaps.validate()?;
        let mut clips = Vec::new();
        let mut gaps_ms = Vec::new();
        let mut missing = None;
        for (index, entry) in lesson.entries.iter().enumerate().skip(start) {
            let Some(clip) = &entry.audio else {
                missing = Some(index);
                break;
            };
            let data = STANDARD
                .decode(clip)
                .map_err(|e| format!("Entry {} has invalid audio: {e}", index + 1))?;
            let decoder =
                crate::audio::wav_decoder(data).map_err(|e| format!("Entry {}: {e}", index + 1))?;
            if !clips.is_empty() {
                gaps_ms
                    .push(gaps.between(lesson.entries[index - 1].participant, entry.participant));
            }
            clips.push(decoder);
        }
        if clips.is_empty() {
            return Err(format!("Entry {} has no recording.", start + 1));
        }
        Ok(Self {
            clips,
            gaps_ms,
            missing,
        })
    }

    fn enqueue(self, player: &rodio::Player) -> (usize, Option<usize>) {
        let count = self.clips.len();
        for (index, clip) in self.clips.into_iter().enumerate() {
            if index > 0 {
                let samples = self.gaps_ms[index - 1] as usize * 48;
                player.append(rodio::source::Zero::new_samples(
                    NonZeroU16::new(1).unwrap(),
                    NonZeroU32::new(48_000).unwrap(),
                    samples,
                ));
            }
            player.append(clip);
        }
        (count, self.missing)
    }
}

// Each clip and each intervening silence is a separate queued source. Select
// the next entry as soon as its predecessor completes, including during gaps.
fn current_index(start: usize, clips: usize, remaining_sources: usize) -> usize {
    let total = clips * 2 - 1;
    let finished = total.saturating_sub(remaining_sources);
    start + ((finished + 1) / 2).min(clips - 1)
}

pub struct Player {
    pub lesson: usize,
    start: usize,
    clips: usize,
    pub selected: usize,
    missing: Option<usize>,
    _output: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl Player {
    pub fn start(
        lesson_index: usize,
        lesson: &Lesson,
        start: usize,
        gaps: AudioGaps,
    ) -> Result<Self, String> {
        let plan = Plan::new(lesson, start, gaps)?;
        let mut output = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Could not open output device: {e}"))?;
        output.log_on_drop(false);
        let player = rodio::Player::connect_new(output.mixer());
        player.pause();
        let (count, missing) = plan.enqueue(&player);
        player.play();
        Ok(Self {
            lesson: lesson_index,
            start,
            clips: count,
            selected: start,
            missing,
            _output: output,
            player,
        })
    }

    pub fn toggle(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }
    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }
    pub fn update(&mut self) -> (usize, bool, Option<String>) {
        let remaining = self.player.len();
        self.selected = current_index(self.start, self.clips, remaining);
        let finished = remaining == 0;
        let error = if finished {
            self.missing
                .map(|i| format!("Playback stopped: entry {} has no recording.", i + 1))
        } else {
            None
        };
        (self.selected, finished, error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Entry, Language, Participant};

    fn clip() -> String {
        let mut wav = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..480 {
            writer.write_sample(1000i16).unwrap();
        }
        writer.finalize().unwrap();
        STANDARD.encode(wav.into_inner())
    }

    fn lesson() -> Lesson {
        let entries = [
            Participant::Teacher,
            Participant::Teacher,
            Participant::Student,
            Participant::Teacher,
        ]
        .into_iter()
        .map(|participant| Entry {
            participant,
            language: Language::Latin,
            text: String::new(),
            audio: Some(clip()),
        })
        .collect();
        Lesson {
            name: "Test".into(),
            expanded: true,
            entries,
        }
    }

    #[test]
    fn starts_at_requested_entry_and_uses_configured_gaps() {
        let lesson = lesson();
        let gaps = AudioGaps {
            same_participant_ms: 123,
            different_participant_ms: 456,
        };
        let plan = Plan::new(&lesson, 0, gaps).unwrap();
        assert_eq!(plan.clips.len(), 4);
        assert_eq!(plan.gaps_ms, [123, 456, 456]);
        let plan = Plan::new(&lesson, 1, gaps).unwrap();
        assert_eq!(plan.clips.len(), 3);
        assert_eq!(plan.gaps_ms, [456, 456]);
        assert_eq!(plan.missing, None);
    }

    #[test]
    fn incomplete_lessons_stop_at_first_missing_recording() {
        let mut lesson = lesson();
        lesson.entries[2].audio = None;
        let plan = Plan::new(&lesson, 1, AudioGaps::default()).unwrap();
        assert_eq!(plan.clips.len(), 1);
        assert_eq!(plan.missing, Some(2));
        assert!(Plan::new(&lesson, 2, AudioGaps::default()).is_err());
        assert_eq!(
            Plan::new(&lesson, 3, AudioGaps::default())
                .unwrap()
                .clips
                .len(),
            1
        );
        lesson.entries[1].audio = Some("invalid".into());
        assert!(Plan::new(&lesson, 1, AudioGaps::default()).is_err());
    }

    #[test]
    fn selection_advances_at_clip_end_including_during_silence() {
        assert_eq!(
            (0..=5)
                .rev()
                .map(|remaining| current_index(3, 3, remaining))
                .collect::<Vec<_>>(),
            [3, 4, 4, 5, 5, 5]
        );
        assert_eq!(current_index(7, 1, 1), 7);
        assert_eq!(current_index(7, 1, 0), 7);
    }

    #[test]
    fn queued_playback_contains_clips_and_exact_silence() {
        let mut lesson = lesson();
        lesson.entries.truncate(2);
        let (mixer, mut output) = rodio::mixer::mixer(
            NonZeroU16::new(1).unwrap(),
            NonZeroU32::new(48_000).unwrap(),
        );
        let player = rodio::Player::connect_new(&mixer);
        Plan::new(&lesson, 0, AudioGaps::default())
            .unwrap()
            .enqueue(&player);
        let samples: Vec<f32> = output.by_ref().take(480 + 4800 + 480).collect();
        assert_eq!(samples.len(), 5760);
        assert!(samples[..480].iter().all(|s| *s > 0.02));
        assert!(samples[480..5280].iter().all(|s| *s == 0.0));
        assert!(samples[5280..].iter().all(|s| *s > 0.02));
    }
}
