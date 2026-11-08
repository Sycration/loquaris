//! Messages shown as cards stacked in a window's bottom-right corner, over its
//! content, so nothing moves. Each window has its own stack, so a message
//! appears where the action that caused it happened.
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

/// How long an info or success message stays, counted while it isn't hovered.
const LIFETIME: Duration = Duration::from_secs(4);
/// Older messages are dropped beyond this many.
const MAX_TOASTS: usize = 5;
const WIDTH: f32 = 340.0;
const MARGIN: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    /// Stays until dismissed.
    Error,
}

impl Level {
    fn icon(self) -> &'static str {
        match self {
            Level::Info => "ℹ",
            Level::Success => "✔",
            Level::Error => "⚠",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Level::Info => Color32::from_gray(150),
            Level::Success => Color32::from_rgb(110, 200, 120),
            Level::Error => Color32::from_rgb(235, 90, 90),
        }
    }
}

#[derive(Debug)]
struct Toast {
    id: u64,
    level: Level,
    text: String,
    /// How many times the message was raised.
    count: usize,
    /// When the message was raised or last hovered; it expires from then.
    since: Instant,
}

#[derive(Debug, Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    pub fn info(&mut self, text: impl Into<String>) {
        self.push(Level::Info, text.into());
    }

    pub fn success(&mut self, text: impl Into<String>) {
        self.push(Level::Success, text.into());
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(Level::Error, text.into());
    }

    /// Raise a message. A repeat of one still shown is counted and moved to
    /// the bottom instead of stacking a copy.
    pub fn push(&mut self, level: Level, text: String) {
        let now = Instant::now();
        let mut toast = match self
            .toasts
            .iter()
            .position(|toast| toast.level == level && toast.text == text)
        {
            Some(index) => {
                let mut toast = self.toasts.remove(index);
                toast.count += 1;
                toast
            }
            None => {
                self.next_id += 1;
                Toast {
                    id: self.next_id,
                    level,
                    text,
                    count: 1,
                    since: now,
                }
            }
        };
        toast.since = now;
        self.toasts.push(toast);
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    /// The messages shown, oldest first.
    #[cfg(test)]
    pub fn messages(&self) -> Vec<(Level, &str, usize)> {
        self.toasts
            .iter()
            .map(|toast| (toast.level, toast.text.as_str(), toast.count))
            .collect()
    }

    fn expire(&mut self, now: Instant) {
        self.toasts
            .retain(|toast| toast.level == Level::Error || now - toast.since < LIFETIME);
    }

    /// Draw the stack in the bottom-right corner of the viewport being painted.
    pub fn show(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.expire(now);
        if self.toasts.is_empty() {
            return;
        }
        let mut dismissed = None;
        let response = egui::Area::new(egui::Id::unique("toasts"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-MARGIN, -MARGIN])
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 8.0;
                for toast in &self.toasts {
                    if toast_card(ui, toast) {
                        dismissed = Some(toast.id);
                    }
                }
            })
            .response;
        let hovered = response.contains_pointer();
        if let Some(id) = dismissed {
            self.toasts.retain(|toast| toast.id != id);
        }
        // Keep messages while the pointer is over them, so they can be read.
        if hovered {
            for toast in &mut self.toasts {
                toast.since = now;
            }
        }
        if let Some(remaining) = self
            .toasts
            .iter()
            .filter(|toast| toast.level != Level::Error)
            .map(|toast| LIFETIME.saturating_sub(now - toast.since))
            .min()
        {
            ctx.request_repaint_after(remaining);
        }
    }
}

/// Draw one message, returning whether it was dismissed.
fn toast_card(ui: &mut egui::Ui, toast: &Toast) -> bool {
    let mut dismissed = false;
    let color = toast.level.color();
    egui::Frame::new()
        .fill(Color32::from_gray(28))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
        .show(ui, |ui| {
            ui.set_width(WIDTH - 24.0);
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(toast.level.icon()).color(color).size(16.0));
                ui.add(egui::Label::new(RichText::new(&toast.text).color(Color32::WHITE)).wrap());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                    dismissed = true;
                }
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(toast.text.clone());
                }
                if toast.count > 1 {
                    ui.label(RichText::new(format!("×{}", toast.count)).color(color));
                }
            });
        });
    dismissed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_are_counted_and_only_errors_persist() {
        let mut toasts = Toasts::default();
        toasts.error("Disk full");
        toasts.success("Saved");
        toasts.error("Disk full");
        assert_eq!(
            toasts.messages(),
            [(Level::Success, "Saved", 1), (Level::Error, "Disk full", 2)]
        );
        toasts.expire(Instant::now() + LIFETIME);
        assert_eq!(toasts.messages(), [(Level::Error, "Disk full", 2)]);
    }

    #[test]
    fn the_oldest_messages_are_dropped() {
        let mut toasts = Toasts::default();
        for index in 0..=MAX_TOASTS {
            toasts.error(index.to_string());
        }
        assert_eq!(toasts.messages().len(), MAX_TOASTS);
        assert_eq!(toasts.messages()[0].1, "1");
    }
}
