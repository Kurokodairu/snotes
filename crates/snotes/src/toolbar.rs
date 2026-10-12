//! Top toolbar: tool buttons, colour presets, width slider, undo/redo.
//! Talks to the canvas only; no engine types.

use crate::canvas::Canvas;
use crate::engine::{Rgba, ShapeKind, Tool};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use std::cell::Cell;
use std::rc::Rc;

const DEFAULT_WIDTH: f64 = 2.0;
const PRESETS: [(&str, Rgba); 5] = [
    ("black", Rgba(0.0, 0.0, 0.0, 1.0)),
    ("blue", Rgba(0.1, 0.3, 0.9, 1.0)),
    ("red", Rgba(0.85, 0.1, 0.1, 1.0)),
    ("green", Rgba(0.1, 0.6, 0.2, 1.0)),
    ("orange", Rgba(0.95, 0.5, 0.05, 1.0)),
];

/// 48 px targets are big enough to hit with a pen in tablet mode.
const CSS: &str = "
.snotes-toolbar button { min-width: 48px; min-height: 48px; }
.swatch-black { color: #000000; } .swatch-blue { color: #1a4de6; }
.swatch-red { color: #d91a1a; } .swatch-green { color: #1a9933; }
.swatch-orange { color: #f28000; }
.swatch { font-size: 28px; }
";

pub fn build(canvas: &Canvas) -> gtk4::Widget {
    if let Some(display) = gdk::Display::default() {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    bar.add_css_class("snotes-toolbar");
    bar.set_margin_start(6);
    bar.set_margin_end(6);
    bar.set_margin_top(4);
    bar.set_margin_bottom(4);

    let colour = Rc::new(Cell::new(PRESETS[0].1));
    let width = Rc::new(Cell::new(DEFAULT_WIDTH));
    let push_style = {
        let (canvas, colour, width) = (canvas.clone(), colour.clone(), width.clone());
        move || canvas.set_pen_style(colour.get(), width.get())
    };
    push_style();

    // Tools: one radio group.
    let tools: [(&str, Tool); 7] = [
        ("Pen", Tool::Pen),
        ("Marker", Tool::Highlighter),
        ("Eraser", Tool::Eraser),
        ("Line", Tool::Shaper(ShapeKind::Line)),
        ("Rect", Tool::Shaper(ShapeKind::Rect)),
        ("Oval", Tool::Shaper(ShapeKind::Ellipse)),
        ("Lasso", Tool::Lasso),
    ];
    let mut group: Option<gtk4::ToggleButton> = None;
    for (label, tool) in tools {
        let button = gtk4::ToggleButton::with_label(label);
        match &group {
            Some(first) => button.set_group(Some(first)),
            None => {
                button.set_active(true);
                group = Some(button.clone());
            }
        }
        let canvas = canvas.clone();
        button.connect_toggled(move |b| {
            if b.is_active() {
                canvas.set_tool(tool);
            }
        });
        bar.append(&button);
    }

    bar.append(&gtk4::Separator::new(gtk4::Orientation::Vertical));

    // Colour presets: another radio group.
    let mut colours: Option<gtk4::ToggleButton> = None;
    for (name, rgba) in PRESETS {
        let button = gtk4::ToggleButton::with_label("●");
        button.add_css_class("swatch");
        button.add_css_class(&format!("swatch-{name}"));
        match &colours {
            Some(first) => button.set_group(Some(first)),
            None => {
                button.set_active(true);
                colours = Some(button.clone());
            }
        }
        let (colour, push_style) = (colour.clone(), push_style.clone());
        button.connect_toggled(move |b| {
            if b.is_active() {
                colour.set(rgba);
                push_style();
            }
        });
        bar.append(&button);
    }

    let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.5, 20.0, 0.5);
    scale.set_value(DEFAULT_WIDTH);
    scale.set_width_request(160);
    scale.set_valign(gtk4::Align::Center);
    {
        let (width, push_style) = (width.clone(), push_style.clone());
        scale.connect_value_changed(move |s| {
            width.set(s.value());
            push_style();
        });
    }
    bar.append(&scale);

    bar.append(&gtk4::Separator::new(gtk4::Orientation::Vertical));
    let undo = gtk4::Button::with_label("Undo");
    let redo = gtk4::Button::with_label("Redo");
    undo.connect_clicked(glib::clone!(
        #[weak]
        canvas,
        move |_| canvas.undo()
    ));
    redo.connect_clicked(glib::clone!(
        #[weak]
        canvas,
        move |_| canvas.redo()
    ));
    bar.append(&undo);
    bar.append(&redo);

    bar.upcast()
}
