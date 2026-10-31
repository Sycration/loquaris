//! Rendering of the main window and its secondary windows.
//!
//! Each frame draws the chrome (menus and toolbars), the two-column script,
//! and the add-entry bar, then shows any open child windows. Rects of
//! interactive widgets are collected so a double click on bare background can
//! clear the selection.
mod card;
mod chrome;
mod lesson;
mod project_details;
mod script;
mod selection_search;
mod side_panel;
mod welcome;
mod windows;

#[cfg(test)]
pub use card::{EntryMenuOptions, audio_marker_rect, card};

use crate::{Language, app::App};
use eframe::egui::{self, Color32, Stroke, Vec2, text::LayoutJob};
use std::ops::Range;

pub const L1_COLOR: Color32 = Color32::from_rgb(24, 68, 80);
pub const LATIN_COLOR: Color32 = Color32::from_rgb(80, 0, 0);
pub const L1_ACCENT: Color32 = Color32::from_rgb(60, 184, 213);
pub const LATIN_ACCENT: Color32 = Color32::from_rgb(224, 70, 70);
pub const AUDIO_MARKER_COLOR: Color32 = Color32::from_rgb(189, 219, 63);
pub const AUDIO_MARKER_INSET: f32 = 8.0;
/// Colour of search matches, which are also underlined in it.
pub const SEARCH_HIGHLIGHT: Color32 = Color32::YELLOW;
const LATIN_FONT: &str = "source-serif-4";
/// Height reserved below the script for the add-entry bar.
const FOOTER_HEIGHT: f32 = 75.0;

/// Rects of widgets drawn this frame; clicks inside them aren't background clicks.
type Foreground = Vec<egui::Rect>;

pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "ibm-plex-sans".into(),
        egui::FontData::from_static(include_bytes!("../../assets/IBMPlexSans-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        LATIN_FONT.into(),
        egui::FontData::from_static(include_bytes!("../../assets/SourceSerif4-Regular.ttf")).into(),
    );
    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.insert(0, "ibm-plex-sans".into());
    let mut latin = vec![LATIN_FONT.to_owned()];
    latin.extend(proportional.iter().cloned());
    fonts
        .families
        .insert(egui::FontFamily::Name(LATIN_FONT.into()), latin);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        for font in style.text_styles.values_mut() {
            *font = egui::FontId::proportional(15.0);
        }
    });
}

pub fn entry_font(language: Language, size: f32) -> egui::FontId {
    let family = match language {
        Language::L1 => egui::FontFamily::Proportional,
        Language::Latin => egui::FontFamily::Name(LATIN_FONT.into()),
    };
    egui::FontId::new(size, family)
}

/// Restyle `ranges` of a single-format job as search matches. Ranges must be
/// sorted; any that don't fit the text are skipped.
pub fn highlight_matches(job: &mut LayoutJob, ranges: &[Range<usize>]) {
    if ranges.is_empty() || job.sections.len() != 1 {
        return;
    }
    let base = job.sections[0].format.clone();
    let matched = egui::TextFormat {
        color: SEARCH_HIGHLIGHT,
        underline: Stroke::new(1.0, SEARCH_HIGHLIGHT),
        ..base.clone()
    };
    let text = &job.text;
    let section = |range: Range<usize>, format: &egui::TextFormat| egui::text::LayoutSection {
        leading_space: 0.0,
        byte_range: egui::text::ByteIndex(range.start)..egui::text::ByteIndex(range.end),
        format: format.clone(),
    };
    let mut sections = Vec::new();
    let mut position = 0;
    for range in ranges {
        if range.start < position
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            continue;
        }
        if range.start > position {
            sections.push(section(position..range.start, &base));
        }
        sections.push(section(range.clone(), &matched));
        position = range.end;
    }
    if position < text.len() {
        sections.push(section(position..text.len(), &base));
    }
    job.sections = sections;
}

/// Take keyboard focus away from whichever widget holds it.
pub fn surrender_focus(ctx: &egui::Context) {
    ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
}

fn apply_dark_style(ui: &mut egui::Ui) {
    ui.visuals_mut().override_text_color = Some(Color32::WHITE);
    ui.visuals_mut().panel_fill = Color32::BLACK;
}

pub fn background_double_clicked(
    ctx: &egui::Context,
    layer: egui::LayerId,
    viewport: egui::Rect,
    foreground: &[egui::Rect],
) -> bool {
    if egui::Popup::is_any_open(ctx) {
        return false;
    }
    let position = ctx.input(|input| {
        input
            .pointer
            .button_double_clicked(egui::PointerButton::Primary)
            .then(|| input.pointer.interact_pos())
            .flatten()
    });
    position.is_some_and(|position| {
        viewport.contains(position)
            && ctx.layer_id_at(position) == Some(layer)
            && !foreground.iter().any(|rect| rect.contains(position))
    })
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(test)]
        {
            self.rendered_entry_count = 0;
            self.entry_text_rects.clear();
        }
        if !self.project_open {
            self.welcome(ui);
            return;
        }
        self.update_lesson_playback(ui.ctx());
        self.search_settled_selection(ui.ctx());
        let mut file_action = self.handle_shortcuts(ui);
        self.keybind_editor.show(ui.ctx(), &mut self.keybinds);
        apply_dark_style(ui);
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let background_area = ui.max_rect();
        let mut foreground = Foreground::new();

        if let Some(action) = self.menu_bar(ui, &mut foreground) {
            file_action = Some(action);
        }
        // Below the menu bar: the script on the left, the side panel on the right.
        let area = ui.available_rect_before_wrap();
        let width = (area.width() * (1.0 - side_panel::SIDE_PANEL_FRACTION)).round();
        let (script_area, panel_area) = area.split_left_right_at_x(area.left() + width);
        let mut script_ui = ui.new_child(egui::UiBuilder::new().max_rect(script_area));
        self.toolbar(&mut script_ui, width, &mut foreground);
        let bottom = script_ui.available_rect_before_wrap().bottom();
        let viewport = self.script(&mut script_ui, width, &mut foreground);
        let footer_rect = egui::Rect::from_min_max(
            egui::pos2(viewport.left(), bottom - FOOTER_HEIGHT),
            egui::pos2(viewport.right(), bottom),
        );
        self.add_entry_bar(&mut script_ui, footer_rect, &mut foreground);
        self.side_panel(ui, panel_area, &mut foreground);

        if background_double_clicked(ui.ctx(), ui.layer_id(), background_area, &foreground) {
            self.deselect_entries();
            self.editing_entry = None;
            self.focus_entry = None;
        }
        if let Some(action) = self.show_windows(ui.ctx()) {
            file_action = Some(action);
        }
        if let Some(action) = file_action {
            self.run_file_action(action, ui.ctx());
        }
        if ui.input(|input| input.viewport().close_requested()) && !self.confirm_discard_changes() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_are_split_into_highlighted_sections() {
        let text = "Puellā puellam videt.";
        let font = egui::FontId::proportional(15.0);
        let mut job = LayoutJob::simple(text.into(), font, Color32::WHITE, 100.0);
        // The second range overlaps the first and the third isn't a char boundary.
        highlight_matches(&mut job, &[0..7, 3..5, 8..14, 15..30]);
        let sections: Vec<_> = job
            .sections
            .iter()
            .map(|section| {
                let range = section.byte_range.start.0..section.byte_range.end.0;
                (&text[range], section.format.color == SEARCH_HIGHLIGHT)
            })
            .collect();
        assert_eq!(
            sections,
            [
                ("Puellā", true),
                (" ", false),
                ("puella", true),
                ("m videt.", false)
            ]
        );
        assert!(!job.sections[0].format.underline.is_empty());
    }
}
