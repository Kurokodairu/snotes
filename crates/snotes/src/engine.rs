//! Adapter around rnote-engine. Every call into the engine goes through this
//! module, because the upstream API is not stable.

use crate::snap::{SnapDetector, SnapEvent};
use rnote_compose::builders::ShapeBuilderType;
use rnote_compose::penevent::PenEvent;
use rnote_compose::penpath::Element;
use rnote_engine::engine::{EngineConfig, EngineConfigShared};
use rnote_engine::engine::{EngineTask, EngineTaskReceiver};
use rnote_engine::pens::pensconfig::brushconfig::BrushStyle;
use rnote_engine::pens::{PenMode, PenStyle};
use rnote_engine::{Engine, WidgetFlags};
use std::collections::HashSet;
use std::time::Instant;

/// The highlighter is drawn this many times wider than the pen.
const HIGHLIGHTER_WIDTH_FACTOR: f64 = 3.0;

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

/// The tool selected in the toolbar. The stylus eraser end overrides it while touching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Highlighter,
    Eraser,
    Shaper(ShapeKind),
    Lasso,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Rect,
    Ellipse,
}

/// Colour with components in 0.0..=1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl From<Rgba> for rnote_compose::Color {
    fn from(c: Rgba) -> Self {
        rnote_compose::Color::new(
            f64::from(c.0),
            f64::from(c.1),
            f64::from(c.2),
            f64::from(c.3),
        )
    }
}

#[derive(Clone, Copy)]
enum PenEventKind {
    Down,
    Up,
    Proximity,
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
    config: EngineConfigShared,
    tool: Tool,
    snap: SnapDetector,
    /// A stroke is currently down.
    stroking: bool,
    /// The live stroke has been snapped to a line (the shaper tool is active).
    snapped: bool,
}

impl InkEngine {
    pub fn new() -> Self {
        let mut inner = Engine::default();
        let config = EngineConfigShared::from(EngineConfig::default());
        let _ = inner.install_config(&config, None);
        Self {
            inner,
            config,
            tool: Tool::Pen,
            snap: SnapDetector::new(),
            stroking: false,
            snapped: false,
        }
    }

    #[cfg(test)]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn set_tool(&mut self, tool: Tool) -> Update {
        self.tool = tool;
        let style = {
            let mut config = self.config.write();
            let pens = &mut config.pens_config;
            match tool {
                Tool::Pen => {
                    pens.brush_config.style = BrushStyle::Solid;
                    PenStyle::Brush
                }
                Tool::Highlighter => {
                    pens.brush_config.style = BrushStyle::Marker;
                    PenStyle::Brush
                }
                Tool::Eraser => PenStyle::Eraser,
                Tool::Shaper(kind) => {
                    pens.shaper_config.builder_type = match kind {
                        ShapeKind::Line => ShapeBuilderType::Line,
                        ShapeKind::Rect => ShapeBuilderType::Rectangle,
                        ShapeKind::Ellipse => ShapeBuilderType::Ellipse,
                    };
                    PenStyle::Shaper
                }
                Tool::Lasso => PenStyle::Selector,
            }
        };
        let flags = self.inner.change_pen_style(style);
        flags.into()
    }

    /// Stroke colour and width (logical px) for pen, highlighter and shapes.
    pub fn set_pen_style(&mut self, colour: Rgba, width: f64) -> Update {
        {
            let mut config = self.config.write();
            let pens = &mut config.pens_config;
            let colour = rnote_compose::Color::from(colour);
            pens.brush_config.solid_options.stroke_color = Some(colour);
            pens.brush_config.solid_options.stroke_width = width;
            pens.brush_config.marker_options.stroke_color = Some(colour);
            pens.brush_config.marker_options.stroke_width = width * HIGHLIGHTER_WIDTH_FACTOR;
            pens.shaper_config.smooth_options.stroke_color = Some(colour);
            pens.shaper_config.smooth_options.stroke_width = width;
        }
        self.inner.reinstall_pen_current_style().into()
    }

    pub fn undo(&mut self, now: Instant) -> Update {
        self.inner.undo(now).into()
    }

    pub fn redo(&mut self, now: Instant) -> Update {
        self.inner.redo(now).into()
    }

    /// Number of undo steps available. Stands in for a stroke count in tests,
    /// because the engine's store keys are not public.
    #[cfg(test)]
    fn undo_depth(&mut self) -> usize {
        let now = Instant::now();
        let mut n = 0;
        while self.inner.can_undo() {
            let _ = self.inner.undo(now);
            n += 1;
        }
        for _ in 0..n {
            let _ = self.inner.redo(now);
        }
        n
    }

    /// Current document size in engine units, as (width, height).
    #[cfg_attr(feature = "gui", allow(dead_code))]
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
        let snappable = tool == PenTool::Pen && matches!(self.tool, Tool::Pen | Tool::Highlighter);
        if !self.stroking {
            self.stroking = true;
            self.snap.reset();
        }
        let mut update = self.send(PenEventKind::Down, sample, tool, now);
        if snappable && !self.snapped {
            if let SnapEvent::Snapped { start } = self.snap.push(sample, now) {
                update.redraw |= self.begin_snapped_line(start, sample, tool, now).redraw;
            }
        }
        update
    }

    pub fn pen_up(&mut self, sample: PenSample, tool: PenTool, now: Instant) -> Update {
        let update = self.send(PenEventKind::Up, sample, tool, now);
        self.end_stroke(update)
    }

    pub fn pen_proximity(&mut self, sample: PenSample, tool: PenTool, now: Instant) -> Update {
        // The canvas reports a lifted stylus as proximity, never as pen-up, so
        // a snapped line has to be committed here.
        let kind = if self.snapped {
            PenEventKind::Up
        } else {
            PenEventKind::Proximity
        };
        let update = self.send(kind, sample, tool, now);
        self.end_stroke(update)
    }

    fn end_stroke(&mut self, mut update: Update) -> Update {
        self.stroking = false;
        self.snap.reset();
        if self.snapped {
            self.snapped = false;
            update.redraw |= self.set_tool(self.tool).redraw;
        }
        update
    }

    /// Replace the live brush stroke by a shaper line from `start` to `end`.
    fn begin_snapped_line(
        &mut self,
        start: PenSample,
        end: PenSample,
        tool: PenTool,
        now: Instant,
    ) -> Update {
        let mut update: Update = self
            .inner
            .handle_pen_event(PenEvent::Cancel, None, now)
            .1
            .into();
        {
            let mut config = self.config.write();
            let pens = &mut config.pens_config;
            let (colour, width) = match self.tool {
                Tool::Highlighter => (
                    pens.brush_config.marker_options.stroke_color,
                    pens.brush_config.marker_options.stroke_width,
                ),
                _ => (
                    pens.brush_config.solid_options.stroke_color,
                    pens.brush_config.solid_options.stroke_width,
                ),
            };
            pens.shaper_config.builder_type = ShapeBuilderType::Line;
            pens.shaper_config.smooth_options.stroke_color = colour;
            pens.shaper_config.smooth_options.stroke_width = width;
        }
        // Cancel still commits the partial brush stroke; take it back out.
        update.redraw |= self.inner.undo(now).redraw;
        update.redraw |= self.inner.change_pen_style(PenStyle::Shaper).redraw;
        self.snapped = true;
        update.redraw |= self.send(PenEventKind::Down, start, tool, now).redraw;
        update.redraw |= self.send(PenEventKind::Down, end, tool, now).redraw;
        update
    }

    #[cfg(test)]
    fn is_snapped(&self) -> bool {
        self.snapped
    }

    fn send(
        &mut self,
        kind: PenEventKind,
        sample: PenSample,
        tool: PenTool,
        now: Instant,
    ) -> Update {
        let element = self.to_doc(sample);
        let modifier_keys = HashSet::new();
        let event = match kind {
            PenEventKind::Down => PenEvent::Down {
                element,
                modifier_keys,
            },
            PenEventKind::Up => PenEvent::Up {
                element,
                modifier_keys,
            },
            PenEventKind::Proximity => PenEvent::Proximity {
                element,
                modifier_keys,
            },
        };
        self.pen_event(event, tool, now)
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
    use std::time::Duration;

    #[test]
    fn new_engine_has_a_document() {
        let (w, h) = InkEngine::new().document_size();
        assert!(w > 0.0 && h > 0.0);
    }

    fn stroke(e: &mut InkEngine, tool: PenTool, from: (f64, f64), to: (f64, f64)) {
        let t = Instant::now();
        let at = |f: f64| PenSample {
            x: from.0 + (to.0 - from.0) * f,
            y: from.1 + (to.1 - from.1) * f,
            pressure: 0.5,
        };
        e.pen_proximity(at(0.0), tool, t);
        e.pen_down(at(0.0), tool, t);
        for i in 1..=10 {
            e.pen_down(at(f64::from(i) / 10.0), tool, t);
        }
        e.pen_up(at(1.0), tool, t);
    }

    fn active() -> InkEngine {
        let mut e = InkEngine::new();
        e.set_viewport_size(800.0, 600.0);
        e
    }

    #[test]
    fn set_tool_changes_current_tool() {
        let mut e = active();
        e.set_tool(Tool::Shaper(ShapeKind::Rect));
        assert_eq!(e.tool(), Tool::Shaper(ShapeKind::Rect));
        assert_eq!(e.inner.current_pen_style_w_override(), PenStyle::Shaper);
    }

    #[test]
    fn pen_stroke_adds_one_stroke() {
        let mut e = active();
        stroke(&mut e, PenTool::Pen, (100.0, 100.0), (300.0, 120.0));
        assert_eq!(e.undo_depth(), 1);
    }

    #[test]
    fn undo_removes_stroke_redo_restores() {
        let mut e = active();
        stroke(&mut e, PenTool::Pen, (100.0, 100.0), (300.0, 120.0));
        e.undo(Instant::now());
        assert_eq!(e.undo_depth(), 0);
        e.redo(Instant::now());
        assert_eq!(e.undo_depth(), 1);
    }

    #[test]
    fn eraser_end_override_restores_selected_tool() {
        let mut e = active();
        e.set_tool(Tool::Shaper(ShapeKind::Rect));
        stroke(&mut e, PenTool::Eraser, (100.0, 100.0), (200.0, 100.0));
        stroke(&mut e, PenTool::Pen, (100.0, 200.0), (200.0, 260.0));
        assert_eq!(e.inner.current_pen_style_w_override(), PenStyle::Shaper);
        assert_eq!(e.tool(), Tool::Shaper(ShapeKind::Rect));
    }

    #[test]
    fn highlighter_uses_marker_style() {
        let mut e = active();
        e.set_tool(Tool::Highlighter);
        assert_eq!(
            e.config.read().pens_config.brush_config.style,
            BrushStyle::Marker
        );
        e.set_tool(Tool::Pen);
        assert_eq!(
            e.config.read().pens_config.brush_config.style,
            BrushStyle::Solid
        );
    }

    /// Draw a straight line, hold still, then move the end and lift.
    fn snap_stroke(e: &mut InkEngine, lift_with_proximity: bool) -> Instant {
        let t0 = Instant::now();
        let p = |x: f64, y: f64| PenSample {
            x,
            y,
            pressure: 0.6,
        };
        e.pen_proximity(p(100.0, 100.0), PenTool::Pen, t0);
        for i in 0..=20 {
            let at = t0 + Duration::from_millis(10 * i);
            e.pen_down(p(100.0 + 10.0 * i as f64, 100.0), PenTool::Pen, at);
        }
        let held = t0 + Duration::from_millis(200);
        e.pen_down(
            p(300.0, 100.0),
            PenTool::Pen,
            held + Duration::from_millis(400),
        );
        assert!(e.is_snapped());
        let end = held + Duration::from_millis(450);
        e.pen_down(p(300.0, 160.0), PenTool::Pen, end);
        if lift_with_proximity {
            e.pen_proximity(p(300.0, 160.0), PenTool::Pen, end);
        } else {
            e.pen_up(p(300.0, 160.0), PenTool::Pen, end);
        }
        end
    }

    #[test]
    fn held_straight_stroke_becomes_one_line() {
        let mut e = active();
        snap_stroke(&mut e, false);
        assert!(!e.is_snapped());
        assert_eq!(e.undo_depth(), 1);
        assert_eq!(e.tool(), Tool::Pen);
        assert_eq!(e.inner.current_pen_style_w_override(), PenStyle::Brush);
    }

    #[test]
    fn snapped_line_is_one_undo_step() {
        let mut e = active();
        snap_stroke(&mut e, false);
        e.undo(Instant::now());
        assert_eq!(e.undo_depth(), 0);
    }

    #[test]
    fn release_before_hold_commits_normal_stroke() {
        let mut e = active();
        stroke(&mut e, PenTool::Pen, (100.0, 100.0), (300.0, 100.0));
        assert!(!e.is_snapped());
        assert_eq!(e.undo_depth(), 1);
    }

    #[test]
    fn proximity_loss_while_snapped_resets_state() {
        let mut e = active();
        snap_stroke(&mut e, true);
        assert!(!e.is_snapped());
        assert_eq!(e.undo_depth(), 1);
        assert_eq!(e.inner.current_pen_style_w_override(), PenStyle::Brush);
    }

    #[test]
    fn second_stroke_after_snap_starts_fresh() {
        let mut e = active();
        snap_stroke(&mut e, false);
        stroke(&mut e, PenTool::Pen, (100.0, 300.0), (300.0, 340.0));
        assert!(!e.is_snapped());
        assert_eq!(e.undo_depth(), 2);
    }
}
