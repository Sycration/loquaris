#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 400.0])
            .with_title("loquaris"),
        ..Default::default()
    };

    eframe::run_native(
        "loquaris",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}

#[derive(Default)]
struct App {
    label: String,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            ui.heading("loquaris");
            ui.horizontal(|ui| {
                ui.label("Say something:");
                ui.text_edit_singleline(&mut self.label);
                if ui.button("Greet").clicked() {
                    self.label = format!("Hello, {}!", self.label);
                }
            });
        ui.separator();
        ui.label(&self.label);
    }
}
