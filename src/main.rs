#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod audio_devices;
mod export_pdf;
mod export_wav;
mod keybinds;
mod lesson_player;
mod macronize;
mod model;
mod parsing_review;
mod project;
mod recent_projects;
mod search;
mod toasts;
mod ui;
mod waveform;
mod word_index;

use eframe::egui;
pub use model::{AudioState, Entry, Language, Lesson, Participant, Trim};

fn main() -> eframe::Result {
    eframe::run_native(
        "loquaris",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1200.0, 750.0])
                .with_min_inner_size([630.0, 350.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            ui::configure_fonts(&cc.egui_ctx);
            let mut app = app::App::default();
            app.keybinds = keybinds::Registry::load_user();
            app.recent_projects = recent_projects::RecentProjects::load_user();
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod profile;
#[cfg(test)]
mod tests;
