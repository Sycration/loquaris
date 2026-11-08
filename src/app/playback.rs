//! Whole-lesson playback, with the selection updated to the current entry.
use super::App;
use crate::{lesson_player, model::lesson_recording_summary};
use eframe::egui;

impl App {
    /// Start playing `lesson` from the selected entry, or pause/resume it.
    pub fn toggle_lesson_playback(&mut self, lesson: usize) {
        if let Some(player) = &self.lesson_playback
            && player.lesson == lesson
        {
            player.toggle();
            return;
        }
        self.lesson_playback = None;
        let start = self
            .selected_entry
            .filter(|(li, _)| *li == lesson)
            .map(|(_, index)| index)
            .unwrap_or(0);
        match lesson_player::Player::start(lesson, &self.lessons[lesson], start, self.audio_gaps) {
            Ok(player) => {
                self.follow_playback((lesson, start));
                self.lessons[lesson].expanded = true;
                self.lesson_playback = Some(player);
            }
            Err(error) => self.messages().error(error),
        }
    }

    fn follow_playback(&mut self, position: (usize, usize)) {
        self.set_selection(vec![position], Some(position));
        self.active_lesson = position.0;
        self.scroll_to_entry = Some(position);
        self.editing_entry = None;
    }

    /// Advance the selection with playback; selecting elsewhere stops it.
    pub fn update_lesson_playback(&mut self, ctx: &egui::Context) {
        let Some(player) = &mut self.lesson_playback else {
            return;
        };
        if self.selected_entry != Some((player.lesson, player.selected)) {
            self.lesson_playback = None;
            return;
        }
        let lesson = player.lesson;
        let old = player.selected;
        let (index, finished, error) = player.update();
        let paused = player.is_paused();
        if index != old {
            self.follow_playback((lesson, index));
        }
        if finished {
            self.lesson_playback = None;
            if let Some(error) = error {
                self.toasts.error(error);
            }
        } else if !paused {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
        }
    }

    pub fn recording_summaries(&mut self) -> &[String] {
        self.lesson_recording_summaries.get_or_insert_with(|| {
            self.lessons
                .iter()
                .map(|lesson| lesson_recording_summary(lesson, self.audio_gaps))
                .collect()
        })
    }

    /// Whether the given lesson is playing (`Some(true)`), paused, or idle.
    pub fn playback_state(&self, lesson: usize) -> Option<bool> {
        self.lesson_playback
            .as_ref()
            .filter(|player| player.lesson == lesson)
            .map(|player| !player.is_paused())
    }
}
