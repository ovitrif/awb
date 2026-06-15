#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backend;
mod config;
mod glyph;
mod login_item;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--render-icon") {
        let path = args
            .get(index + 1)
            .map(String::as_str)
            .unwrap_or("icon.png");
        let size = args
            .get(index + 2)
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(1024);
        std::fs::write(path, glyph::app_icon_png(size)).expect("write icon");
        println!("wrote {path} ({size}x{size})");
        return Ok(());
    }

    if let Some(index) = args.iter().position(|arg| arg == "--render-shell") {
        let path = args
            .get(index + 1)
            .map(String::as_str)
            .unwrap_or("shell.png");
        std::fs::write(path, glyph::shell_background_png(3)).expect("write shell");
        println!("wrote {path}");
        return Ok(());
    }

    let viewport = egui::ViewportBuilder::default()
        .with_title("awb")
        .with_inner_size([theme::WINDOW_WIDTH, theme::WINDOW_FULL_HEIGHT])
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_visible(false);

    #[allow(unused_mut)]
    let mut native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

        native_options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }));
    }

    eframe::run_native(
        "awb",
        native_options,
        Box::new(|cc| {
            app::App::new(cc)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("failed to start awb-app: {error:#}").into()
                })
        }),
    )
}
