//! The panel to the right of the student column: search and word study.
//!
//! Top to bottom: the search box, Collo's analysis of the query, the Latin
//! lemmas new in the current lesson, a chart of uses per lesson, and the
//! uses in the current lesson in context.
use super::{Foreground, entry_font};
use crate::{
    Language, Lesson,
    app::App,
    search::{self, Hit, SearchMode},
};
use eframe::egui::{
    self, Color32, CornerRadius, RichText, Sense, Stroke, TextFormat, Vec2, text::LayoutJob,
};
use std::ops::Range;
use std::sync::Arc;

/// Share of the window width below the menu bar given to the panel. A third
/// makes it about as wide as each participant column.
pub const SIDE_PANEL_FRACTION: f32 = 1.0 / 3.0;

const SURFACE: Color32 = Color32::from_gray(18);
const MUTED: Color32 = Color32::from_gray(150);
/// Series colours for the chart: categorical slots 1 and 2, validated for
/// colour-vision deficiency and contrast against `SURFACE`.
const TEACHER_SERIES: Color32 = Color32::from_rgb(0x39, 0x87, 0xe5);
const STUDENT_SERIES: Color32 = Color32::from_rgb(0xd9, 0x59, 0x26);
const CHART_HEIGHT: f32 = 110.0;

fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title).size(13.0).strong().color(MUTED));
    ui.add_space(2.0);
}

fn weak(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED));
}

/// Morphology descriptions are available in fewer languages than translations.
fn morphology_language(translation: &str) -> &'static str {
    match translation {
        "fr" => "fr",
        "es" => "es",
        _ => "en",
    }
}

impl App {
    pub(super) fn side_panel(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        foreground: &mut Foreground,
    ) {
        // Treat the whole panel as foreground so double clicks on it don't
        // clear the script selection.
        foreground.push(rect);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, SURFACE);
        painter.line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, Color32::GRAY),
        );
        let mut panel = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(12.0, 4.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .id_salt("side_panel")
            .auto_shrink([false, false])
            .show(&mut panel, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                let hits = self.search_section(ui);
                let lesson = self.active_lesson.min(self.lessons.len().saturating_sub(1));
                self.analysis_section(ui);
                self.new_lemmas_section(ui, lesson);
                self.uses_chart_section(ui, &hits, lesson);
                self.contexts_section(ui, &hits, lesson);
                ui.add_space(12.0);
            });
    }

    /// The search box and a count of results, which it returns.
    fn search_section(&mut self, ui: &mut egui::Ui) -> Arc<[Hit]> {
        section_heading(ui, "Search");
        ui.horizontal(|ui| {
            let clear_width = if self.search.query.is_empty() {
                0.0
            } else {
                28.0
            };
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search.query)
                    .hint_text("Search the project…")
                    .desired_width((ui.available_width() - clear_width).max(40.0)),
            );
            if edit.changed() {
                self.search.query_changed();
            }
            if !self.search.query.is_empty()
                && ui.button("✕").on_hover_text("Clear search").clicked()
            {
                self.search.set_query(String::new());
            }
        });
        let mut unmarked_short = self.search.unmarked_short();
        if ui
            .checkbox(&mut unmarked_short, "Unmarked vowels are short")
            .on_hover_text(
                "Read a vowel without a macron as short when some parse allows it, \
                 so “malae” is mălus, not māla. Off: only written macrons and \
                 breves rule parses out.",
            )
            .changed()
        {
            self.search.set_unmarked_short(unmarked_short);
        }
        let hits = self
            .search
            .hits(&self.lessons, &self.word_index, self.content_generation);
        if !self.search.is_empty() {
            let lessons = {
                let mut lessons: Vec<_> = hits.iter().map(|hit| hit.lesson).collect();
                lessons.dedup();
                lessons.len()
            };
            let uses = match hits.len() {
                1 => "1 use".to_owned(),
                n => format!("{n} uses"),
            };
            let what = match self.search.mode() {
                SearchMode::Lemma(lemma) => format!("of the lemma {}", lemma.headword),
                SearchMode::Text => "of this text".to_owned(),
            };
            let across = match lessons {
                0 => String::new(),
                1 => " in 1 lesson".to_owned(),
                n => format!(" in {n} lessons"),
            };
            weak(ui, &format!("{uses} {what}{across}"));
        }
        hits
    }

    /// Collo's parses of each query word. For a single word, choosing a lemma
    /// (or exact text) changes what is searched.
    fn analysis_section(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, "Analysis");
        if self.search.is_empty() {
            weak(
                ui,
                "Search for a word or phrase to see how Collo parses it.",
            );
            return;
        }
        let translation = search::translation_language(&self.l1_name);
        let morphology = morphology_language(translation);
        let single_word = self.search.analysis().len() == 1;
        let mode = self.search.mode();
        let mut chosen = None;
        for (index, word) in self.search.analysis().iter().enumerate() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(&word.text)
                    .font(entry_font(Language::Latin, 16.0))
                    .color(Color32::WHITE),
            );
            match self.search.ruled_out().get(index).copied().unwrap_or(0) {
                0 => {}
                count => {
                    ui.label(
                        RichText::new(format!(
                            "{count} other reading{} ruled out by vowel length",
                            if count == 1 { "" } else { "s" }
                        ))
                        .size(12.0)
                        .color(MUTED),
                    )
                    .on_hover_text(if self.search.unmarked_short() {
                        "Written macrons and breves must match, and unmarked vowels \
                         are read as short when some reading allows it."
                    } else {
                        "Written macrons and breves must match."
                    });
                }
            }
            let lemmas = search::lemma_options(word);
            if lemmas.is_empty() {
                weak(ui, "Not recognised as Latin.");
                continue;
            }
            for lemma in lemmas {
                let mut heading = LayoutJob::default();
                heading.append(
                    lemma.headword,
                    0.0,
                    TextFormat::simple(entry_font(Language::Latin, 15.0), Color32::WHITE),
                );
                if let Some(meaning) = lemma.translation(translation) {
                    heading.append(
                        &format!("  {meaning}"),
                        0.0,
                        TextFormat::simple(egui::FontId::proportional(13.0), MUTED),
                    );
                }
                heading.wrap.max_rows = 1;
                heading.wrap.max_width = ui.available_width();
                if single_word {
                    let current = mode == SearchMode::Lemma(lemma);
                    if ui
                        .selectable_label(current, heading)
                        .on_hover_text("Search every form of this lemma")
                        .clicked()
                    {
                        chosen = Some(SearchMode::Lemma(lemma));
                    }
                } else {
                    ui.label(heading);
                }
                let analyses = word
                    .analyses
                    .iter()
                    .enumerate()
                    .filter(|(_, analysis)| std::ptr::eq(analysis.lemma, lemma));
                for (index, analysis) in analyses {
                    let likely = word.selected == Some(index);
                    let description = analysis.morphology(morphology).unwrap_or("");
                    let text = format!(
                        "{}{}{} — {description}",
                        if likely { "★ " } else { "    " },
                        analysis.form,
                        analysis.suffix,
                    );
                    let label = ui.label(RichText::new(text).size(12.0).color(if likely {
                        Color32::WHITE
                    } else {
                        MUTED
                    }));
                    if likely {
                        label.on_hover_text("Collo's most likely reading in context");
                    }
                }
            }
        }
        if single_word
            && ui
                .selectable_label(mode == SearchMode::Text, "Exact text instead")
                .on_hover_text("Search for this spelling in every entry, Latin or not")
                .clicked()
        {
            chosen = Some(SearchMode::Text);
        }
        if let Some(mode) = chosen {
            self.search.set_mode(mode);
        }
    }

    /// Lemmas first used in the current lesson; clicking one searches it.
    fn new_lemmas_section(&mut self, ui: &mut egui::Ui, lesson: usize) {
        section_heading(ui, &format!("New in lesson {}", lesson + 1));
        let lemmas = self.word_index.new_lemmas_in_lesson(lesson);
        if lemmas.is_empty() {
            weak(ui, "No Latin words are introduced in this lesson.");
            return;
        }
        let translation = search::translation_language(&self.l1_name);
        let mode = self.search.mode();
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for lemma in lemmas {
                let text = RichText::new(lemma.headword).font(entry_font(Language::Latin, 14.0));
                let mut response = ui.selectable_label(mode == SearchMode::Lemma(lemma), text);
                if let Some(meaning) = lemma.translation(translation) {
                    response = response.on_hover_text(meaning);
                }
                if response.clicked() {
                    chosen = Some(lemma);
                }
            }
        });
        if let Some(lemma) = chosen {
            self.search.search_lemma(lemma);
        }
    }

    /// A stacked bar per lesson: uses by the teacher and by the student.
    fn uses_chart_section(&mut self, ui: &mut egui::Ui, hits: &[Hit], current: usize) {
        section_heading(ui, "Uses per lesson");
        if self.search.is_empty() || self.lessons.is_empty() {
            weak(ui, "Search to chart how often a word is used.");
            return;
        }
        ui.horizontal(|ui| {
            for (color, label) in [(TEACHER_SERIES, "Teacher"), (STUDENT_SERIES, "Student")] {
                let (swatch, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, color);
                ui.label(RichText::new(label).size(12.0).color(MUTED));
                ui.add_space(8.0);
            }
        });
        let counts = search::counts_per_lesson(&self.lessons, hits);
        if let Some(lesson) = uses_chart(ui, &self.lessons, &counts, current) {
            match hits.iter().find(|hit| hit.lesson == lesson) {
                Some(hit) => self.reveal_entry((hit.lesson, hit.entry)),
                None => {
                    self.active_lesson = lesson;
                    self.scroll_to_lesson = Some(lesson);
                }
            }
            ui.ctx().request_repaint();
        }
    }

    /// Each entry of the current lesson that uses the search, with matches
    /// highlighted and a button to jump to the entry in the script.
    fn contexts_section(&mut self, ui: &mut egui::Ui, hits: &[Hit], lesson: usize) {
        section_heading(ui, &format!("Contexts in lesson {}", lesson + 1));
        if self.search.is_empty() {
            weak(ui, "Search to see each use in this lesson.");
            return;
        }
        let in_lesson: Vec<_> = hits.iter().filter(|hit| hit.lesson == lesson).collect();
        if in_lesson.is_empty() {
            weak(ui, "Not used in this lesson.");
            return;
        }
        let mut reveal = None;
        for group in in_lesson.chunk_by(|a, b| a.entry == b.entry) {
            let entry = group[0].entry;
            let Some(text) = self.lessons[lesson].entries.get(entry).map(|e| &e.text) else {
                continue;
            };
            let ranges: Vec<_> = group.iter().map(|hit| hit.range.clone()).collect();
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                let button_width = 28.0;
                let text_width = (ui.available_width() - button_width - 6.0).max(40.0);
                ui.allocate_ui(Vec2::new(text_width, 0.0), |ui| {
                    ui.add(egui::Label::new(highlighted(text, &ranges, text_width)).wrap());
                });
                if ui
                    .add(egui::Button::new("→").min_size(Vec2::new(button_width, 24.0)))
                    .on_hover_text("Select this entry in the script")
                    .clicked()
                {
                    reveal = Some((lesson, entry));
                }
            });
        }
        if let Some(position) = reveal {
            self.reveal_entry(position);
            ui.ctx().request_repaint();
        }
    }
}

/// Entry text in the Latin font with each range yellow and underlined.
fn highlighted(text: &str, ranges: &[Range<usize>], width: f32) -> LayoutJob {
    let font = entry_font(Language::Latin, 14.0);
    let mut job = LayoutJob::simple(text.to_owned(), font, Color32::WHITE, width);
    super::highlight_matches(&mut job, ranges);
    job
}

/// Draw the stacked bars, returning a lesson whose bar was clicked.
fn uses_chart(
    ui: &mut egui::Ui,
    lessons: &[Lesson],
    counts: &[[usize; 2]],
    current: usize,
) -> Option<usize> {
    let max = counts.iter().map(|[t, s]| t + s).max().unwrap_or(0);
    let axis_width = 26.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), CHART_HEIGHT + 18.0),
        Sense::click(),
    );
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + axis_width, rect.top() + 6.0),
        egui::pos2(rect.right(), rect.top() + 6.0 + CHART_HEIGHT),
    );
    let small = egui::FontId::proportional(11.0);
    painter.text(
        egui::pos2(plot.left() - 6.0, plot.top()),
        egui::Align2::RIGHT_CENTER,
        max.to_string(),
        small.clone(),
        MUTED,
    );
    painter.text(
        egui::pos2(plot.left() - 6.0, plot.bottom()),
        egui::Align2::RIGHT_CENTER,
        "0",
        small.clone(),
        MUTED,
    );
    painter.line_segment(
        [plot.left_bottom(), plot.right_bottom()],
        Stroke::new(1.0, Color32::from_gray(80)),
    );

    let count = lessons.len().max(1) as f32;
    let slot = plot.width() / count;
    let gap = if slot >= 6.0 { 2.0 } else { 0.0 };
    let bar_width = (slot - gap).max(1.0);
    let hovered = response
        .hover_pos()
        .filter(|pos| pos.x >= plot.left())
        .map(|pos| (((pos.x - plot.left()) / slot) as usize).min(lessons.len() - 1));
    for (lesson, &[teacher, student]) in counts.iter().enumerate() {
        let left = plot.left() + lesson as f32 * slot;
        let column = egui::Rect::from_min_max(
            egui::pos2(left, plot.top()),
            egui::pos2(left + bar_width, plot.bottom()),
        );
        if lesson == current || hovered == Some(lesson) {
            let shade = if lesson == current { 40 } else { 28 };
            painter.rect_filled(column, 0.0, Color32::from_gray(shade));
        }
        if max == 0 {
            continue;
        }
        let scale = plot.height() / max as f32;
        let mut bottom = plot.bottom();
        let segments = [(teacher, TEACHER_SERIES), (student, STUDENT_SERIES)];
        let top_segment = segments.iter().rposition(|(n, _)| *n > 0);
        for (index, &(n, color)) in segments.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let height = n as f32 * scale;
            let top = bottom - height;
            let radius = if Some(index) == top_segment {
                (4.0_f32).min(bar_width / 2.0).min(height) as u8
            } else {
                0
            };
            let segment = egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(left + bar_width, bottom),
            );
            painter.rect_filled(
                segment,
                CornerRadius {
                    nw: radius,
                    ne: radius,
                    sw: 0,
                    se: 0,
                },
                color,
            );
            // A surface gap separates stacked segments.
            bottom = top - if gap > 0.0 { 2.0 } else { 0.0 };
        }
    }
    let label_y = plot.bottom() + 9.0;
    painter.text(
        egui::pos2(plot.left(), label_y),
        egui::Align2::LEFT_CENTER,
        "Lesson 1",
        small.clone(),
        MUTED,
    );
    if lessons.len() > 1 {
        painter.text(
            egui::pos2(plot.right(), label_y),
            egui::Align2::RIGHT_CENTER,
            lessons.len().to_string(),
            small,
            MUTED,
        );
    }
    let mut response = response;
    if let Some(lesson) = hovered {
        let [teacher, student] = counts[lesson];
        response = response.on_hover_text(format!(
            "Lesson {}: {}\nTeacher {teacher} · Student {student}\nClick to go to the first use",
            lesson + 1,
            lessons[lesson].display_name(),
        ));
    }
    if response.clicked() { hovered } else { None }
}
