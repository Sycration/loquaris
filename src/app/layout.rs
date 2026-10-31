//! Estimated row heights, so off-screen lessons and entries can be skipped.
use super::App;
use crate::{Language, Participant, ui};
use eframe::egui::{self, Color32};

/// Left margin and width of an entry card within its participant's column.
pub fn entry_column_geometry(column_width: f32, participant: Participant) -> (f32, f32) {
    let margin = 10.0
        + if participant == Participant::Student {
            ui::AUDIO_MARKER_INSET
        } else {
            0.0
        };
    (
        margin,
        (column_width - 20.0 - ui::AUDIO_MARKER_INSET).max(20.0),
    )
}

pub fn estimated_entry_row_height(
    text: &str,
    text_width: f32,
    line_height: f32,
    label_height: f32,
    average_char_width: f32,
    interaction_height: f32,
) -> f32 {
    let lines = text
        .split('\n')
        .map(|line| {
            let characters = line
                .chars()
                .filter(|character| !unicode_normalization::char::is_combining_mark(*character))
                .count();
            ((characters as f32 * average_char_width * 1.12) / text_width)
                .ceil()
                .max(1.0)
        })
        .sum::<f32>();
    let text_height = (lines * line_height).max(interaction_height);
    // Frame stroke and padding, language label and gap, top offset and row gap.
    2.0 + 12.0 + label_height + 6.0 + text_height + 7.0
}

impl App {
    /// Re-estimate every row when the script shape, width, or scale changed.
    /// Rendered rows later overwrite their estimate with the measured height.
    pub fn ensure_entry_row_heights(&mut self, ui: &egui::Ui, width: f32) {
        let dimensions_match = self.entry_row_heights.len() == self.lessons.len()
            && self
                .entry_row_heights
                .iter()
                .zip(&self.lessons)
                .all(|(heights, lesson)| heights.len() == lesson.entries.len());
        let pixels_per_point = ui.ctx().pixels_per_point();
        if dimensions_match
            && (self.layout_cache_width - width).abs() < 0.5
            && (self.layout_cache_pixels_per_point - pixels_per_point).abs() < 0.01
        {
            return;
        }

        let column_width = (width - 2.0) / 2.0;
        let (_, card_width) = entry_column_geometry(column_width, Participant::Teacher);
        let text_width = (card_width - 12.0).max(8.0);
        let spacing = ui.spacing().extra_text_line_spacing;
        let interaction_height = ui.spacing().interact_size.y;
        let [l1_metrics, latin_metrics] = ui.fonts_mut(|fonts| {
            [Language::L1, Language::Latin].map(|language| {
                let font = ui::entry_font(language, 15.0);
                let label_font = ui::entry_font(language, 12.0);
                let sample = fonts.layout_no_wrap(
                    "abcdefghijklmnopqrstuvwxyz".into(),
                    font.clone(),
                    Color32::WHITE,
                );
                (
                    fonts.row_height(&font) + spacing,
                    fonts.row_height(&label_font),
                    sample.size().x / 26.0,
                )
            })
        });

        self.entry_row_heights = self
            .lessons
            .iter()
            .map(|lesson| {
                lesson
                    .entries
                    .iter()
                    .map(|entry| {
                        let (line_height, label_height, average_char_width) = match entry.language {
                            Language::L1 => l1_metrics,
                            Language::Latin => latin_metrics,
                        };
                        estimated_entry_row_height(
                            &entry.text,
                            text_width,
                            line_height,
                            label_height,
                            average_char_width,
                            interaction_height,
                        )
                    })
                    .collect()
            })
            .collect();
        self.layout_cache_width = width;
        self.layout_cache_pixels_per_point = pixels_per_point;
    }

    pub fn estimated_lesson_height(&self, lesson: usize) -> f32 {
        let item = &self.lessons[lesson];
        let header = 32.0;
        if !item.expanded {
            return header;
        }
        if item.entries.is_empty() {
            return header + 22.0 + 26.0;
        }
        header + 2.0 + self.entry_row_heights[lesson].iter().sum::<f32>() + 26.0
    }
}
