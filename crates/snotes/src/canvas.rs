//! GTK widget that hosts the ink engine: forwards pen input in, draws the
//! engine out. Phase 0 spike, no page/row model yet.

use crate::engine::{PenSample, PenTool, Update};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, graphene, Native};
use std::time::{Duration, Instant};

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>)
        @extends gtk4::Widget;
}

impl Canvas {
    pub fn new() -> Self {
        glib::Object::new()
    }

    fn apply(&self, update: Update) {
        if update.redraw {
            self.queue_draw();
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

/// Index of an axis inside `gdk::TimeCoord::axes()` (gtk-rs has no helper).
fn axis_idx(axis: gdk::AxisUse) -> usize {
    match axis {
        gdk::AxisUse::X => 1,
        gdk::AxisUse::Y => 2,
        gdk::AxisUse::Pressure => 5,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PenState {
    #[default]
    Up,
    Proximity,
    Down,
}

mod imp {
    use super::*;
    use crate::engine::InkEngine;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct Canvas {
        pub engine: RefCell<InkEngine>,
        pub pen_state: Cell<PenState>,
        /// Print every raw pen event to stderr (set `SNOTES_LOG_PEN=1`).
        pub log_pen: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "SnotesCanvas";
        type Type = super::Canvas;
        type ParentType = gtk4::Widget;
    }

    impl ObjectImpl for Canvas {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            self.log_pen
                .set(std::env::var_os("SNOTES_LOG_PEN").is_some());

            // Background tasks from the engine (stroke rendering etc.).
            if let Some(mut rx) = self.engine.borrow_mut().take_task_receiver() {
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    obj,
                    async move {
                        while let Some(task) = rx.recv().await {
                            let (update, quit) = obj.imp().engine.borrow_mut().handle_task(task);
                            obj.apply(update);
                            if quit {
                                break;
                            }
                        }
                    }
                ));
            }

            let controller = gtk4::EventControllerLegacy::new();
            controller.connect_event(glib::clone!(
                #[weak]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, event| obj.imp().handle_event(event)
            ));
            obj.add_controller(controller);
        }
    }

    impl WidgetImpl for Canvas {
        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let obj = self.obj();
            let mut engine = self.engine.borrow_mut();
            let update = engine.set_scale_factor(f64::from(obj.scale_factor()));
            let update2 = engine.set_viewport_size(f64::from(width), f64::from(height));
            drop(engine);
            obj.apply(update);
            obj.apply(update2);
        }

        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let obj = self.obj();
            let (w, h) = (f64::from(obj.width()), f64::from(obj.height()));
            snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, w as f32, h as f32));
            if let Err(e) = self.engine.borrow().draw(snapshot, w, h) {
                eprintln!("snapshot failed: {e}");
            }
            snapshot.pop();
        }
    }

    impl Canvas {
        fn handle_event(&self, event: &gdk::Event) -> glib::Propagation {
            let now = Instant::now();
            let stylus = event.device_tool().is_some();
            let tool = match event.device_tool().map(|t| t.tool_type()) {
                Some(gdk::DeviceToolType::Eraser) => PenTool::Eraser,
                _ => PenTool::Pen,
            };
            let mut state = self.pen_state.get();

            use gdk::EventType as E;
            match event.event_type() {
                E::MotionNotify => {
                    if stylus {
                        // Same trick as gtkgesturestylus: no reliable tip events.
                        state = if event
                            .modifier_state()
                            .contains(gdk::ModifierType::BUTTON1_MASK)
                        {
                            PenState::Down
                        } else {
                            PenState::Proximity
                        };
                    }
                }
                E::ButtonPress => {
                    if event.downcast_ref::<gdk::ButtonEvent>().map(|b| b.button())
                        != Some(gdk::BUTTON_PRIMARY)
                    {
                        return glib::Propagation::Proceed;
                    }
                    state = PenState::Down;
                    self.obj().grab_focus();
                }
                E::ButtonRelease => {
                    state = if stylus {
                        PenState::Proximity
                    } else {
                        PenState::Up
                    };
                }
                E::ProximityIn => state = PenState::Proximity,
                E::ProximityOut => state = PenState::Up,
                // Touch is for panning and zooming later, never ink.
                _ => return glib::Propagation::Proceed,
            }
            if !stylus && event.is_pointer_emulated() {
                return glib::Propagation::Proceed;
            }

            let Some(samples) = self.samples(event, now, stylus) else {
                return glib::Propagation::Proceed;
            };

            let mut update = Update::default();
            {
                let mut engine = self.engine.borrow_mut();
                for (sample, at) in samples {
                    let u = match state {
                        PenState::Down => engine.pen_down(sample, tool, at),
                        PenState::Proximity => engine.pen_proximity(sample, tool, at),
                        PenState::Up => engine.pen_up(sample, tool, at),
                    };
                    update.redraw |= u.redraw;
                }
            }
            self.pen_state.set(state);
            self.obj().apply(update);
            glib::Propagation::Stop
        }

        /// Samples for one gdk event: the coalesced history first, then the
        /// event itself, each with its timestamp.
        fn samples(
            &self,
            event: &gdk::Event,
            now: Instant,
            stylus: bool,
        ) -> Option<Vec<(PenSample, Instant)>> {
            let obj = self.obj();
            let native = Native::for_surface(&event.surface()?)?;
            let (tx, ty) = native.surface_transform();
            let to_widget = |x: f64, y: f64| -> Option<(f64, f64)> {
                let p = native.compute_point(
                    &*obj,
                    &graphene::Point::new((x - tx) as f32, (y - ty) as f32),
                )?;
                Some((f64::from(p.x()), f64::from(p.y())))
            };

            let event_time = event.time();
            let mut out = Vec::new();
            let mut history_len = 0;

            if event.event_type() == gdk::EventType::MotionNotify {
                for entry in event.history() {
                    let axes = entry.axes();
                    let delta =
                        Duration::from_millis(u64::from(event_time.saturating_sub(entry.time())));
                    let Some(at) = now.checked_sub(delta) else {
                        continue;
                    };
                    let Some((x, y)) = to_widget(
                        axes[axis_idx(gdk::AxisUse::X)],
                        axes[axis_idx(gdk::AxisUse::Y)],
                    ) else {
                        continue;
                    };
                    let pressure = if stylus {
                        axes[axis_idx(gdk::AxisUse::Pressure)]
                    } else {
                        0.5
                    };
                    out.push((PenSample { x, y, pressure }, at));
                    history_len += 1;
                }
            }

            let (ex, ey) = event.position()?;
            let (x, y) = to_widget(ex, ey)?;
            let pressure = if stylus {
                event.axis(gdk::AxisUse::Pressure).unwrap_or(0.0)
            } else {
                0.5
            };
            out.push((PenSample { x, y, pressure }, now));

            if self.log_pen.get() {
                eprintln!(
                    "pen type={:?} gdk_time_ms={} x={:.1} y={:.1} pressure={:.3} stylus={} tool={:?} history={}",
                    event.event_type(),
                    event_time,
                    x,
                    y,
                    pressure,
                    stylus,
                    event.device_tool().map(|t| t.tool_type()),
                    history_len,
                );
            }
            Some(out)
        }
    }
}
