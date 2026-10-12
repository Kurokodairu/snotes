//! Adapter around rnote-engine. Every call into the engine goes through this
//! module, because the upstream API is not stable.

use rnote_compose::penevent::PenEvent;
use rnote_compose::penpath::Element;
use rnote_engine::engine::{EngineTask, EngineTaskReceiver};
use rnote_engine::pens::PenMode;
use rnote_engine::{Engine, WidgetFlags};
use std::collections::HashSet;
use std::time::Instant;

/// One pen sample in widget coordinates (logical pixels, origin top-left).
#[derive(Debug, Clone, Copy)]
pub struct PenSample {
    pub x: f64,
    pub y: f64,
    /// 0.0..=1.0
    pub pressure: f64,
}

/// Which end of the stylus is touching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenTool {
    Pen,
    Eraser,
}

/// What the widget has to do after an engine call.
#[derive(Debug, Clone, Copy, Default)]
pub struct Update {
    pub redraw: bool,
}

impl From<WidgetFlags> for Update {
    fn from(flags: WidgetFlags) -> Self {
        Self {
            redraw: flags.redraw,
        }
    }
}

/// Owns the rnote engine instance behind SnoteS' own interface.
pub struct InkEngine {
    inner: Engine,
}

impl InkEngine {
    pub fn new() -> Self {
        Self {
            inner: Engine::default(),
        }
    }

    /// Current document size in engine units, as (width, height).
    pub fn document_size(&self) -> (f64, f64) {
        (self.inner.document.width, self.inner.document.height)
    }
}

impl InkEngine {
    /// Take the receiver for background tasks (stroke rendering, autosave...).
    /// Feed each task to [`InkEngine::handle_task`]. Returns `None` after the first call.
    pub fn take_task_receiver(&mut self) -> Option<EngineTaskReceiver> {
        self.inner.take_engine_tasks_rx()
    }

    /// Handle a background task. Returns the redraw request and whether the
    /// task loop should stop.
    pub fn handle_task(&mut self, task: EngineTask) -> (Update, bool) {
        let (flags, quit) = self.inner.handle_engine_task(task);
        (flags.into(), quit)
    }

    /// Activate the engine and fit it to a widget of the given size.
    pub fn set_viewport_size(&mut self, width: f64, height: f64) -> Update {
        let flags = self.inner.set_active(true)
            | self.inner.camera_set_size(nalgebra::vector![width, height])
            | self.inner.update_rendering_current_viewport();
        flags.into()
    }

    pub fn set_scale_factor(&mut self, scale_factor: f64) -> Update {
        self.inner.set_scale_factor(scale_factor).into()
    }

    /// Widget coordinates to document coordinates.
    fn to_doc(&self, sample: PenSample) -> Element {
        let doc = self.inner.camera.transform().inverse() * nalgebra::point![sample.x, sample.y];
        Element::new(doc.coords, sample.pressure)
    }

    pub fn pen_down(&mut self, sample: PenSample, tool: PenTool, now: Instant) -> Update {
        let element = self.to_doc(sample);
        self.pen_event(
            PenEvent::Down {
                element,
                modifier_keys: HashSet::new(),
            },
            tool,
            now,
        )
    }

    pub fn pen_up(&mut self, sample: PenSample, tool: PenTool, now: Instant) -> Update {
        let element = self.to_doc(sample);
        self.pen_event(
            PenEvent::Up {
                element,
                modifier_keys: HashSet::new(),
            },
            tool,
            now,
        )
    }

    pub fn pen_proximity(&mut self, sample: PenSample, tool: PenTool, now: Instant) -> Update {
        let element = self.to_doc(sample);
        self.pen_event(
            PenEvent::Proximity {
                element,
                modifier_keys: HashSet::new(),
            },
            tool,
            now,
        )
    }

    fn pen_event(&mut self, event: PenEvent, tool: PenTool, now: Instant) -> Update {
        let mode = match tool {
            PenTool::Pen => PenMode::Pen,
            PenTool::Eraser => PenMode::Eraser,
        };
        let (_propagation, flags) = self.inner.handle_pen_event(event, Some(mode), now);
        flags.into()
    }

    /// Draw the whole engine (background, strokes, live stroke) into a GTK snapshot.
    #[cfg(feature = "gui")]
    pub fn draw(&self, snapshot: &gtk4::Snapshot, width: f64, height: f64) -> Result<(), String> {
        use rnote_compose::ext::AabbExt;

        let bounds =
            AabbExt::new_positive(nalgebra::point![0.0, 0.0], nalgebra::point![width, height]);
        self.inner
            .draw_to_gtk_snapshot(snapshot, bounds)
            .map_err(|e| e.to_string())
    }
}

impl Default for InkEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_has_a_document() {
        let (w, h) = InkEngine::new().document_size();
        assert!(w > 0.0 && h > 0.0);
    }
}
