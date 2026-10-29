//! A contextual parsing chooser. Collo exposes one preferred analysis, not a ranking.
use eframe::egui::{self, RichText};

use crate::macronize;

pub struct Review {
    pub lesson: usize,
    pub entry: usize,
    pub original: String,
    words: Vec<collo::tagging::TaggedWord>,
    choices: Vec<Option<usize>>,
    selected_word: Option<usize>,
}

pub enum Outcome {
    Cancel,
    Apply(String),
}

impl Review {
    pub fn new(lesson: usize, entry: usize, original: String) -> Self {
        let words = collo::Lemmatizer::new().tag_text(&original);
        let choices = words
            .iter()
            .map(|word| {
                word.selected.filter(|&index| {
                    word.analyses.get(index).is_some_and(|analysis| {
                        macronize::analysis_word(&word.text, analysis, true).is_some()
                    })
                })
            })
            .collect();
        let selected_word = words.iter().position(|word| !word.analyses.is_empty());
        Self {
            lesson,
            entry,
            original,
            words,
            choices,
            selected_word,
        }
    }

    fn word_preview(&self, index: usize) -> String {
        let word = &self.words[index];
        self.choices[index]
            .and_then(|choice| word.analyses.get(choice))
            .and_then(|analysis| macronize::analysis_word(&word.text, analysis, true))
            .unwrap_or_else(|| word.text.clone())
    }

    pub fn preview(&self) -> String {
        let mut text = String::with_capacity(self.original.len());
        let mut cursor = 0;
        for (index, word) in self.words.iter().enumerate() {
            text.push_str(&self.original[cursor..word.range.start]);
            text.push_str(&self.word_preview(index));
            cursor = word.range.end;
        }
        text.push_str(&self.original[cursor..]);
        text
    }

    /// Return a result only when every available reading of every word produces
    /// the same safe spelling. Unknown words stay unchanged.
    pub fn unambiguous_preview(&self) -> Option<String> {
        let mut text = String::with_capacity(self.original.len());
        let mut cursor = 0;
        for word in &self.words {
            text.push_str(&self.original[cursor..word.range.start]);
            let mut result = None;
            for analysis in &word.analyses {
                let spelling = macronize::analysis_word(&word.text, analysis, true)?;
                if result
                    .as_ref()
                    .is_some_and(|previous| *previous != spelling)
                {
                    return None;
                }
                result = Some(spelling);
            }
            text.push_str(result.as_deref().unwrap_or(&word.text));
            cursor = word.range.end;
        }
        text.push_str(&self.original[cursor..]);
        Some(text)
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let mut outcome = None;
        let width = (ctx.content_rect().width() - 60.0).clamp(250.0, 720.0);
        let list_height = (ctx.content_rect().height() - 300.0).clamp(80.0, 280.0);
        egui::Modal::new(egui::Id::unique("parsing_review")).show(ctx, |ui| {
            ui.set_width(width);
            ui.heading("Review Latin parsings");
            ui.label("Collo's contextual choice is marked; other readings are not ranked.");
            ui.add_space(8.0);
            ui.label("Choose a word:");
            egui::ScrollArea::vertical().id_salt("review_words").max_height(70.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for index in 0..self.words.len() {
                        let label = RichText::new(self.word_preview(index)).font(crate::entry_font(crate::Language::Latin, 15.0));
                        if ui.selectable_label(self.selected_word == Some(index), label).clicked() { self.selected_word = Some(index); }
                    }
                });
            });
            ui.add_space(8.0);
            if let Some(index) = self.selected_word {
                let word = &self.words[index];
                ui.label(RichText::new(format!("Readings for {}", word.text)).strong());
                if word.analyses.is_empty() {
                    ui.label("No dictionary analyses; the original spelling will be kept.");
                } else {
                    egui::ScrollArea::vertical().id_salt("review_analyses").max_height(list_height).show(ui, |ui| {
                        if ui.selectable_label(self.choices[index].is_none(), "Keep original spelling").clicked() { self.choices[index] = None; }
                        let mut indices: Vec<_> = (0..word.analyses.len()).collect();
                        if let Some(preferred) = word.selected && preferred < indices.len() {
                            indices.swap(0, preferred);
                        }
                        for candidate in indices {
                            let analysis = &word.analyses[candidate];
                            let spelling = macronize::analysis_word(&word.text, analysis, true);
                            let description = analysis.morphology("en").unwrap_or("unknown morphology");
                            let preferred = if word.selected == Some(candidate) { " · collo's choice" } else { "" };
                            let mut label = egui::text::LayoutJob::default();
                            let latin = egui::TextFormat { font_id: crate::entry_font(crate::Language::Latin, 15.0), color: ui.visuals().text_color(), ..Default::default() };
                            let default = egui::TextFormat { font_id: egui::FontId::proportional(15.0), color: ui.visuals().text_color(), ..Default::default() };
                            label.append(spelling.as_deref().unwrap_or("Spelling cannot be aligned"), 0.0, if spelling.is_some() { latin.clone() } else { default.clone() });
                            label.append(" — ", 0.0, default.clone());
                            label.append(analysis.lemma.headword, 0.0, latin);
                            label.append(&format!(" · {description}{preferred}"), 0.0, default);
                            let response = ui.add_enabled(spelling.is_some(), egui::Button::selectable(self.choices[index] == Some(candidate), label).wrap());
                            if response.clicked() { self.choices[index] = Some(candidate); }
                            if spelling.is_none() { response.on_hover_text("Collo reconstructed a different spelling; this reading cannot be applied safely."); }
                        }
                    });
                }
            } else {
                ui.label("No analyzable words found.");
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() { outcome = Some(Outcome::Apply(self.preview())); }
                if ui.button("Cancel").clicked() { outcome = Some(Outcome::Cancel); }
            });
        });
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uses_choices_and_preserves_separators() {
        let mut review = Review::new(0, 0, "Puellas, xyzzy!\npuellas.".into());
        assert_eq!(review.preview(), "Puellās, xyzzy!\npuellās.");
        review.choices[0] = None;
        assert_eq!(review.preview(), "Puellas, xyzzy!\npuellās.");
    }

    #[test]
    fn only_unambiguous_macronizations_skip_review() {
        assert_eq!(
            Review::new(0, 0, "puellas".into())
                .unambiguous_preview()
                .as_deref(),
            Some("puellās")
        );
        // Multiple grammatical analyses can still have the same spelling.
        assert_eq!(
            Review::new(0, 0, "amicas".into())
                .unambiguous_preview()
                .as_deref(),
            Some("amīcās")
        );
        assert_eq!(
            Review::new(0, 0, "malum".into()).unambiguous_preview(),
            None
        );
        assert_eq!(
            Review::new(0, 0, "xyzzy".into())
                .unambiguous_preview()
                .as_deref(),
            Some("xyzzy")
        );
    }

    #[test]
    fn choosing_another_reading_can_replace_existing_macrons() {
        let mut review = Review::new(0, 0, "mālum".into());
        let analyses = &review.words[0].analyses;
        let short = analyses
            .iter()
            .position(|analysis| analysis.form.contains('ă'))
            .unwrap();
        let long = analyses
            .iter()
            .position(|analysis| analysis.form.contains('ā'))
            .unwrap();
        review.choices[0] = Some(short);
        assert_eq!(review.preview(), "malum");
        review.choices[0] = Some(long);
        assert_eq!(review.preview(), "mālum");
        review.choices[0] = None;
        assert_eq!(review.preview(), "mālum");
    }
}
