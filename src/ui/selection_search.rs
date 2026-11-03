//! Searching for text selected in the transcript. Only entries that aren't
//! being edited count; selecting in the entry editor doesn't search.
//!
//! egui reports a finished label selection only as an output command at the
//! very end of a pass, after the app has drawn. This plugin captures it so the
//! search can run on it in the next frame. The command doesn't say which label the selection
//! is in, and egui also sends it for a plain click (with the label's whole
//! text), so the app tracks whether the transcript text was really selected:
//! dragged across or double- or triple-clicked since the last press.
use crate::app::App;
use eframe::egui;
use unicode_normalization::char::is_combining_mark;

#[derive(Default)]
struct SettledSelection {
    text: Option<String>,
}

impl egui::Plugin for SettledSelection {
    fn debug_name(&self) -> &'static str {
        "loquaris::SettledSelection"
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        for command in &output.platform_output.commands {
            if let egui::OutputCommand::TextSelectionSettled(text) = command {
                self.text = Some(text.clone());
                ctx.request_repaint();
            }
        }
    }
}

/// The label selection settled during the previous frame, if any.
fn take_settled(ctx: &egui::Context) -> Option<String> {
    let Some(plugin) = ctx.plugin_opt::<SettledSelection>() else {
        ctx.add_plugin(SettledSelection::default());
        return None;
    };
    plugin.lock().text.take()
}

/// The search for a selection: surrounding spaces and punctuation are dropped,
/// and selections spanning several entries or lines are ignored.
pub fn selection_query(text: &str) -> Option<&str> {
    let query = text.trim_matches(|c: char| !(c.is_alphanumeric() || is_combining_mark(c)));
    (!query.is_empty() && !query.contains('\n') && query.chars().count() <= 100).then_some(query)
}

impl App {
    /// Search a label selection from last frame if it started in the transcript.
    /// Call before drawing the script, which records selecting on entry text.
    pub(super) fn search_settled_selection(&mut self, ctx: &egui::Context) {
        if let Some(text) = take_settled(ctx)
            && self.transcript_text_selecting
        {
            self.search_selection(&text);
        }
        if ctx.input(|input| input.pointer.any_pressed()) {
            self.transcript_text_selecting = false;
        }
    }

    pub fn search_selection(&mut self, text: &str) {
        if let Some(query) = selection_query(text)
            && query != self.search.query
        {
            self.search.set_query(query.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_are_trimmed_to_words_and_phrases() {
        assert_eq!(selection_query(" manūs, "), Some("manūs"));
        assert_eq!(selection_query("“duae manūs”."), Some("duae manūs"));
        // A trailing combining macron belongs to the word.
        assert_eq!(selection_query("mana\u{304}."), Some("mana\u{304}"));
        assert_eq!(selection_query(" , "), None);
        assert_eq!(selection_query("manus\nduae"), None);
    }
}
