// Without the gui feature the pen/draw half of the adapter has no caller.
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

mod engine;
mod snap;

#[cfg(feature = "gui")]
mod canvas;

#[cfg(feature = "gui")]
fn main() -> gtk4::glib::ExitCode {
    use gtk4::prelude::*;

    let app = gtk4::Application::builder()
        .application_id("dev.snotes.Spike")
        .build();
    app.connect_activate(|app| {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("SnoteS spike")
            .default_width(1000)
            .default_height(700)
            .child(&canvas::Canvas::new())
            .build();
        window.present();
    });
    app.run()
}

#[cfg(not(feature = "gui"))]
fn main() {
    let engine = engine::InkEngine::new();
    println!(
        "SnoteS {} (document: {:?}); build with --features gui for the window",
        env!("CARGO_PKG_VERSION"),
        engine.document_size()
    );
}
