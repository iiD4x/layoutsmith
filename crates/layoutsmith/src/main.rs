//! layoutsmith: a visual editor for Android layout XML files.

mod app;
mod canvas;
mod inspector;
mod outline;
mod project;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("layoutsmith")
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "layoutsmith",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))),
    )
}
