//! The pane's frame path (the seam's "The frame handle"): the one view that
//! paints the renderer's frames. Its paint takes the newest frame from the
//! [`LivePane`] (latest wins; taking lets the next draw start), drops a
//! frame of another view without uploading it, turns the frame's BGRA rows
//! into what GPUI paints ([`upload`], the one function that reads a
//! frame's pixels), releases the previous image, and draws the newest one
//! at the pane's height's scale, centred, on black. Its prepaint tells the
//! pane its physical size and scale at every layout change. The waker only
//! notifies this view, so a frame repaints the picture and nothing else;
//! an idle pane is woken by nothing.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::state::live;
use crate::{CameraState, Frame, LivePane, PaneSize, TimeOfDay, ViewKey};

/// The newest frame painted: its image and what it showed.
pub struct Shown {
    /// The image GPUI paints.
    pub image: Arc<RenderImage>,
    /// The frame's counter (a pick names it).
    pub seq: u64,
    /// Its view.
    pub view: ViewKey,
    /// Its revision.
    pub revision: u64,
    /// Its size, physical pixels.
    pub size: [u32; 2],
    /// The overlays' anchors in it, physical pixels.
    pub anchors: Vec<Option<[f32; 2]>>,
    /// Its camera.
    pub camera: CameraState,
    /// Its time of day.
    pub time: TimeOfDay,
}

/// What a paint cost, for the speed runs (`--log`'s `pane` line).
#[derive(Clone, Debug, Default)]
pub struct Costs {
    /// Each new frame's take, copy and paint on the interface thread.
    pub uploads: Vec<Duration>,
    /// Frames dropped unuploaded (another view's).
    pub dropped: u64,
    /// Paints with no new frame.
    pub repaints: u64,
    /// From a camera input given to the pane to the paint of the first
    /// frame after it (the paint's start, before the present).
    pub input_to_paint: Vec<Duration>,
    /// When the last eight new frames were painted, with their counters
    /// and ticks' revisions (the idle measurement says which frame came).
    pub recent: std::collections::VecDeque<(Instant, u64)>,
    /// From a frame's arrival (the renderer's wake) to the window taking
    /// it at its paint: the window's wait for its next frame.
    pub waits: Vec<Duration>,
    /// From a take to the next frame's arrival: the renderer's draw and
    /// readback of the next frame (it starts at the take).
    pub renders: Vec<Duration>,
    /// The last take.
    pub last_take: Option<Instant>,
}

/// The pane's handle with the time of the oldest camera input not yet
/// shown, for [`Costs::input_to_paint`]; every call goes through.
pub struct Timed {
    /// The renderer's pane.
    pub inner: Arc<dyn LivePane>,
    /// When the oldest input not yet painted was given.
    pub since: std::sync::Mutex<Option<Instant>>,
    /// When the frame waiting to be taken arrived (the first wake since
    /// the last take).
    pub arrived: Arc<std::sync::Mutex<Option<Instant>>>,
}

impl LivePane for Timed {
    fn set_waker(&self, wake: Box<dyn Fn() + Send + Sync>) {
        let arrived = self.arrived.clone();
        self.inner.set_waker(Box::new(move || {
            arrived
                .lock()
                .expect("the arrival's lock")
                .get_or_insert_with(Instant::now);
            wake()
        }))
    }
    fn resize(&self, size: PaneSize) {
        self.inner.resize(size)
    }
    fn visible(&self, visible: bool) {
        self.inner.visible(visible)
    }
    fn camera(&self, input: crate::CameraInput) {
        self.since
            .lock()
            .expect("the input clock")
            .get_or_insert_with(Instant::now);
        self.inner.camera(input)
    }
    fn overlays(&self, overlays: crate::Overlays) {
        self.inner.overlays(overlays)
    }
    fn time_of_day(&self, time: TimeOfDay) {
        self.inner.time_of_day(time)
    }
    fn pick(&self, seq: u64, at: [f32; 2], answer: Box<dyn FnOnce(crate::Pick) + Send>) {
        self.inner.pick(seq, at, answer)
    }
    fn take(&self) -> Option<Frame> {
        self.inner.take()
    }
    fn status(&self) -> crate::PaneStatus {
        self.inner.status()
    }
    fn camera_state(&self) -> CameraState {
        self.inner.camera_state()
    }
    fn facts(&self) -> crate::LiveFacts {
        self.inner.facts()
    }
}

/// What a painted frame tells the pane.
pub type OnFrame = Rc<dyn Fn(&mut App)>;
/// What the locked pointer's motion tells the pane.
pub type OnMotion = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

/// The frame path's state, shared by the pane and its frame view.
#[derive(Default)]
pub struct Store {
    /// The pane's handle, once the core gave it.
    pub pane: Option<Arc<dyn LivePane>>,
    /// The view the window shows; a frame of another is dropped.
    pub view: Option<ViewKey>,
    /// The newest painted frame.
    pub shown: Option<Shown>,
    /// The pane's bounds in the window, logical pixels, and the scale.
    pub bounds: Option<(Bounds<Pixels>, f32)>,
    /// The size last told to the pane.
    pub told: Option<PaneSize>,
    /// What painting cost.
    pub costs: Costs,
    /// Told when a frame of the shown view was painted first or anew.
    pub on_frame: Option<OnFrame>,
    /// Told of the locked pointer's motion.
    pub on_motion: Option<OnMotion>,
    /// The pane's timed handle (the same as `pane`).
    pub input_since: Option<Arc<Timed>>,
}

impl Store {
    /// The pane's size in physical pixels.
    pub fn physical(&self) -> Option<[u32; 2]> {
        let (bounds, scale) = self.bounds?;
        Some([
            (f32::from(bounds.size.width) * scale).round() as u32,
            (f32::from(bounds.size.height) * scale).round() as u32,
        ])
    }

    /// A window position as physical pixels from the pane's top left.
    pub fn at(&self, position: Point<Pixels>) -> Option<[f32; 2]> {
        let (bounds, scale) = self.bounds?;
        Some([
            f32::from(position.x - bounds.origin.x) * scale,
            f32::from(position.y - bounds.origin.y) * scale,
        ])
    }

    /// A pane position in the shown frame's pixels and its counter, for a
    /// pick; none with no frame or outside it.
    pub fn in_frame(&self, at: [f32; 2]) -> Option<(u64, [f32; 2])> {
        let shown = self.shown.as_ref()?;
        let pane = self.physical()?;
        live::in_frame(at, shown.size, pane).map(|p| (shown.seq, p))
    }

    /// An anchor of the shown frame as a window position, logical pixels.
    pub fn anchor(&self, index: usize) -> Option<Point<Pixels>> {
        let shown = self.shown.as_ref()?;
        let at = (*shown.anchors.get(index)?)?;
        let (bounds, scale) = self.bounds?;
        let pane = self.physical()?;
        let [x, y, w, h] = live::placed(shown.size, pane);
        let px_ = x + at[0] / shown.size[0] as f32 * w;
        let py_ = y + at[1] / shown.size[1] as f32 * h;
        Some(point(
            bounds.origin.x + px(px_ / scale),
            bounds.origin.y + px(py_ / scale),
        ))
    }
}

/// The frame's rows as a GPUI image: the cheapest route GPUI offers for a
/// buffer that changes every frame, one copy of the rows into an image
/// buffer (GPUI's [`RenderImage`] is BGRA, the frame's byte order, so the
/// rows go in unchanged), uploaded to the sprite atlas by the paint that
/// first draws it.
pub fn upload(frame: &Frame) -> Arc<RenderImage> {
    let [w, h] = frame.size;
    let buffer = image::RgbaImage::from_raw(w, h, frame.bgra().to_vec())
        .expect("a frame's pixels are its width times its height times 4 bytes");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// The view that paints the frames.
pub struct FrameView {
    /// The shared state.
    pub store: Rc<RefCell<Store>>,
}

impl Render for FrameView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let layout = self.store.clone();
        let paint = self.store.clone();
        canvas(
            move |bounds, window, _| {
                let scale = window.scale_factor();
                let mut store = layout.borrow_mut();
                store.bounds = Some((bounds, scale));
                let [width, height] = store.physical().expect("bounds just set");
                let size = PaneSize {
                    width,
                    height,
                    scale,
                };
                if store.told != Some(size) {
                    store.told = Some(size);
                    if let Some(pane) = &store.pane {
                        pane.resize(size);
                    }
                }
            },
            move |bounds, _, window, cx| {
                let started = Instant::now();
                let mut store = paint.borrow_mut();
                // The locked pointer's motion has no position: the window
                // hands it to every listener, and the pane takes it.
                if let Some(on_motion) = store.on_motion.clone() {
                    window.on_mouse_event(move |event: &MouseMotionEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            on_motion(event.delta, window, cx);
                        }
                    });
                }
                let taken = store.pane.as_ref().and_then(|p| p.take());
                let mut fresh = false;
                if taken.is_some() {
                    let took = Instant::now();
                    let arrived = store
                        .input_since
                        .as_ref()
                        .and_then(|t| t.arrived.lock().expect("the arrival's lock").take());
                    if let Some(arrived) = arrived {
                        store
                            .costs
                            .waits
                            .push(took.saturating_duration_since(arrived));
                        if let Some(last) = store.costs.last_take {
                            store
                                .costs
                                .renders
                                .push(arrived.saturating_duration_since(last));
                        }
                    }
                    store.costs.last_take = Some(took);
                }
                if let Some(frame) = taken {
                    if store.view.as_ref() == Some(&frame.view) {
                        let image = upload(&frame);
                        let old = store.shown.replace(Shown {
                            image,
                            seq: frame.seq,
                            view: frame.view.clone(),
                            revision: frame.revision,
                            size: frame.size,
                            anchors: frame.anchors.clone(),
                            camera: frame.camera,
                            time: frame.time,
                        });
                        if let Some(old) = old {
                            window
                                .drop_image(old.image)
                                .expect("the sprite atlas lets an image go");
                        }
                        fresh = true;
                    } else {
                        store.costs.dropped += 1;
                    }
                } else {
                    store.costs.repaints += 1;
                }
                let scale = window.scale_factor();
                if let (Some(shown), Some(pane)) = (&store.shown, store.physical())
                    && store.view.as_ref() == Some(&shown.view)
                {
                    let [x, y, w, h] = live::placed(shown.size, pane);
                    let image_bounds = Bounds {
                        origin: point(
                            bounds.origin.x + px(x / scale),
                            bounds.origin.y + px(y / scale),
                        ),
                        size: size(px(w / scale), px(h / scale)),
                    };
                    window
                        .paint_image(
                            bounds,
                            image_bounds,
                            Corners::default(),
                            shown.image.clone(),
                            0,
                            false,
                        )
                        .expect("the sprite atlas takes the frame");
                }
                if fresh {
                    store.costs.uploads.push(started.elapsed());
                    let seq = store.shown.as_ref().map_or(0, |s| s.seq);
                    store.costs.recent.push_back((started, seq));
                    if store.costs.recent.len() > 8 {
                        store.costs.recent.pop_front();
                    }
                    if let Some(since) = store
                        .input_since
                        .as_ref()
                        .and_then(|t| t.since.lock().expect("the input clock").take())
                    {
                        store
                            .costs
                            .input_to_paint
                            .push(started.saturating_duration_since(since));
                    }
                    if let Some(on_frame) = store.on_frame.clone() {
                        cx.defer(move |cx| on_frame(cx));
                    }
                }
            },
        )
        .size_full()
    }
}
