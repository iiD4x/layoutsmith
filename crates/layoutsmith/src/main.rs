//! layoutsmith: a visual editor for Android layout XML files.

// Release builds are GUI apps on Windows: no console window behind the editor.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod canvas;
mod inspector;
mod outline;
mod project;

use std::path::PathBuf;

use eframe::egui;

fn run(renderer: eframe::Renderer, path: Option<PathBuf>) -> eframe::Result {
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        renderer,
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_title("layoutsmith")
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    // Direct3D 12 rather than Vulkan, including the WARP software renderer on
    // virtual machines and Remote Desktop, and the integrated GPU on hybrid
    // laptops. WGPU_BACKEND and WGPU_POWER_PREF still override this.
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        use eframe::wgpu::{Backends, PowerPreference};
        setup.instance_descriptor.backends =
            Backends::from_env().unwrap_or(Backends::DX12 | Backends::GL);
        setup.power_preference = PowerPreference::from_env().unwrap_or(PowerPreference::LowPower);
    }
    eframe::run_native(
        "layoutsmith",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))),
    )
}

fn main() {
    // Without a console a panic would close the window without a word.
    #[cfg(all(windows, not(debug_assertions)))]
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default_hook(info);
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("layoutsmith crashed")
                .set_description(info.to_string())
                .show();
        }));
    }

    // Absolute, so the file dialogs and res/values lookup work no matter
    // how the path was typed.
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p));

    // LAYOUTSMITH_RENDERER=glow|wgpu overrides the platform default.
    let forced = std::env::var("LAYOUTSMITH_RENDERER")
        .ok()
        .and_then(|r| r.parse::<eframe::Renderer>().ok());
    #[allow(unused_mut)]
    let mut result = run(forced.unwrap_or_default(), path.clone());
    // No usable Direct3D/OpenGL adapter for wgpu: try plain OpenGL before
    // giving up. eframe reuses its event loop, so running again is allowed.
    #[cfg(windows)]
    if forced.is_none() && matches!(result, Err(eframe::Error::Wgpu(_))) {
        result = run(eframe::Renderer::Glow, path);
    }

    if let Err(err) = result {
        let text = format!(
            "layoutsmith could not start:\n\n{err}\n\n\
             Updating the graphics driver usually helps. You can also try the other \
             renderer by setting the environment variable LAYOUTSMITH_RENDERER=glow or wgpu."
        );
        eprintln!("{text}");
        #[cfg(windows)]
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("layoutsmith")
            .set_description(text)
            .show();
        std::process::exit(1);
    }
}
