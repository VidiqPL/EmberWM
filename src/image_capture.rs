//! ext-image-copy-capture: isolated window and display capture, SHM, bounded and on demand.
use crate::{
    render::{Scene, SceneElement},
    screencast::{write_pixels, Readback},
    Ember,
};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::OutputDamageTracker,
            element::AsRenderElements,
            gles::{GlesRenderbuffer, GlesRenderer},
            utils::RendererSurfaceStateUserData,
            Bind, Offscreen, Renderer,
        },
    },
    desktop::Window,
    output::Output,
    reexports::{
        wayland_protocols::ext::{
            image_capture_source::v1::server::{
                ext_foreign_toplevel_image_capture_source_manager_v1::{
                    self, ExtForeignToplevelImageCaptureSourceManagerV1,
                },
                ext_image_capture_source_v1::ExtImageCaptureSourceV1,
                ext_output_image_capture_source_manager_v1::{
                    self, ExtOutputImageCaptureSourceManagerV1,
                },
            },
            image_copy_capture::v1::server::{
                ext_image_copy_capture_cursor_session_v1::{
                    self, ExtImageCopyCaptureCursorSessionV1,
                },
                ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
                ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1},
                ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
            },
        },
        wayland_server::{
            protocol::{wl_buffer::WlBuffer, wl_output, wl_shm, wl_surface::WlSurface},
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum, Weak,
        },
    },
    utils::{IsAlive, Logical, Rectangle, Size, Transform},
    wayland::{
        compositor::with_states,
        foreign_toplevel_list::{
            ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
        },
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Default)]
pub enum Source {
    Output(Output),
    Window(WlSurface),
    #[default]
    Unavailable,
}
pub struct Policy(pub Arc<AtomicBool>);
fn visible(client: &Client, policy: &Policy) -> bool {
    policy.0.load(Ordering::Relaxed) || crate::screencast::trusted_portal(client)
}
fn permitted(state: &Ember, client: &Client) -> bool {
    state.config.screencast.enabled
        && state.session_active
        && (state.config.screencast.allow_direct_capture
            || crate::screencast::trusted_portal(client))
}
struct SessionInner {
    size: Option<Size<i32, Logical>>,
    stopped: bool,
    frame: Option<Weak<ExtImageCopyCaptureFrameV1>>,
}
pub struct SessionData {
    source: Source,
    cursor: bool,
    inner: Mutex<SessionInner>,
}
pub struct FrameData {
    session: Arc<SessionData>,
    inner: Mutex<FrameInner>,
}
#[derive(Default)]
struct FrameInner {
    used: bool,
    buffer: Option<WlBuffer>,
}
struct Copy {
    frame: ExtImageCopyCaptureFrameV1,
    buffer: WlBuffer,
    session: Arc<SessionData>,
}
#[derive(Default)]
pub struct Runtime {
    sessions: Vec<Weak<ExtImageCopyCaptureSessionV1>>,
    pending: Vec<Copy>,
    pub frames: u64,
    pub failures: u64,
}
impl Runtime {
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

pub fn init(dh: &DisplayHandle, direct: Arc<AtomicBool>) -> ForeignToplevelListState {
    dh.create_global::<Ember, ExtImageCopyCaptureManagerV1, _>(1, Policy(direct.clone()));
    dh.create_global::<Ember, ExtOutputImageCaptureSourceManagerV1, _>(1, Policy(direct.clone()));
    dh.create_global::<Ember, ExtForeignToplevelImageCaptureSourceManagerV1, _>(
        1,
        Policy(direct.clone()),
    );
    ForeignToplevelListState::new_with_filter::<Ember>(dh, move |c| {
        direct.load(Ordering::Relaxed) || crate::screencast::trusted_portal(c)
    })
}
fn window<'a>(state: &'a Ember, surface: &WlSurface) -> Option<&'a Window> {
    state
        .windows
        .iter()
        .find(|m| m.window.alive() && m.initialized && crate::wm::has_surface(&m.window, surface))
        .map(|m| &m.window)
}
fn source_size(state: &Ember, source: &Source) -> Option<Size<i32, Logical>> {
    let size = match source {
        Source::Output(output) if state.space.outputs().any(|o| o == output) => state.output_size(),
        Source::Window(surface) => {
            let mapped = with_states(surface, |s| {
                s.data_map
                    .get::<RendererSurfaceStateUserData>()
                    .is_some_and(|d| d.lock().unwrap().buffer().is_some())
            });
            if !mapped {
                return None;
            }
            window(state, surface)?.geometry().size
        }
        _ => return None,
    };
    (size.w > 0
        && size.h > 0
        && size.w <= 8192
        && size.h <= 8192
        && i64::from(size.w) * i64::from(size.h) <= 16_777_216)
        .then_some(size)
}
fn constraints(session: &ExtImageCopyCaptureSessionV1, size: Size<i32, Logical>) {
    session.buffer_size(size.w as u32, size.h as u32);
    session.shm_format(wl_shm::Format::Argb8888);
    session.done();
}
fn new_session(
    state: &mut Ember,
    id: New<ExtImageCopyCaptureSessionV1>,
    source: Source,
    cursor: bool,
    init: &mut DataInit<'_, Ember>,
) {
    let size = source_size(state, &source);
    let data = Arc::new(SessionData {
        source,
        cursor,
        inner: Mutex::new(SessionInner {
            size,
            stopped: size.is_none(),
            frame: None,
        }),
    });
    let session = init.init(id, data);
    if let Some(size) = size {
        constraints(&session, size);
    } else {
        session.stopped();
    }
    state.image_capture.sessions.push(session.downgrade());
}

/// Track metadata and unmaps, including windows on other workspaces, without a polling thread.
pub fn refresh(state: &mut Ember) {
    let mut sessions = std::mem::take(&mut state.image_capture.sessions);
    sessions.retain(|weak| {
        let Ok(session) = weak.upgrade() else {
            return false;
        };
        let data = session.data::<Arc<SessionData>>().unwrap();
        let mut inner = data.inner.lock().unwrap();
        if !inner.stopped {
            let size = if state.config.screencast.enabled && state.session_active {
                source_size(state, &data.source)
            } else {
                None
            };
            if let Some(size) = size {
                if Some(size) != inner.size {
                    inner.size = Some(size);
                    constraints(&session, size);
                }
            } else {
                session.stopped();
                inner.stopped = true;
            }
        }
        true
    });
    if sessions.is_empty() {
        sessions = Vec::new();
    }
    state.image_capture.sessions = sessions;
    state
        .image_capture
        .pending
        .retain(|p| p.frame.is_alive() && p.buffer.is_alive());
}

/// Render just the selected source. Other windows and compositor UI cannot enter window frames.
pub fn render(state: &mut Ember, scene: &mut Scene, renderer: &mut GlesRenderer) {
    let copies = std::mem::take(&mut state.image_capture.pending);
    if copies.is_empty() {
        return;
    }
    for copy in copies {
        if !copy.frame.is_alive() {
            continue;
        }
        let size = source_size(state, &copy.session.source);
        let mut inner = copy.session.inner.lock().unwrap();
        if inner.stopped
            || !state.session_active
            || !state.config.screencast.enabled
            || size.is_none()
        {
            copy.frame
                .failed(ext_image_copy_capture_frame_v1::FailureReason::Stopped);
            state.image_capture.failures += 1;
            continue;
        }
        let size = size.unwrap();
        if inner.size != Some(size) {
            // Refresh normally advertises this before capture, but handle a commit in the same dispatch.
            inner.size = Some(size);
            for weak in &state.image_capture.sessions {
                if let Ok(session) = weak.upgrade() {
                    if Arc::ptr_eq(session.data::<Arc<SessionData>>().unwrap(), &copy.session) {
                        constraints(&session, size);
                    }
                }
            }
        }
        drop(inner);
        let elements = match &copy.session.source {
            Source::Output(_) => scene.elements_with_cursor(state, renderer, copy.session.cursor),
            Source::Window(surface) => {
                let w = window(state, surface).unwrap();
                let geo = w.geometry();
                // Negative xdg window-geometry offset puts the content origin at (0,0).
                let mut elements = Vec::<SceneElement>::new();
                if copy.session.cursor && !state.pointer_locked() {
                    if let Some(origin) = state.space.element_location(w) {
                        let pos =
                            state.seat.get_pointer().unwrap().current_location() - origin.to_f64();
                        if Rectangle::from_size(size).to_f64().contains(pos)
                            && state
                                .surface_under(state.seat.get_pointer().unwrap().current_location())
                                .is_some_and(|(s, _)| s.id().same_client_as(&surface.id()))
                        {
                            scene.capture_cursor(state, renderer, pos, &mut elements);
                        }
                    }
                }
                elements.extend(w.render_elements::<SceneElement>(
                    renderer,
                    (-geo.loc.x, -geo.loc.y).into(),
                    1.0.into(),
                    1.0,
                ));
                elements
            }
            Source::Unavailable => unreachable!(),
        };
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut target: GlesRenderbuffer =
                renderer.create_buffer(Fourcc::Abgr8888, (size.w, size.h).into())?;
            let mut framebuffer = renderer.bind(&mut target)?;
            let mut damage = OutputDamageTracker::new((size.w, size.h), 1., Transform::Normal);
            damage.render_output(renderer, &mut framebuffer, 0, &elements, [0., 0., 0., 1.])?;
            let mapping = Readback::capture(renderer, &framebuffer, size.w, size.h, false)?;
            let pixels = mapping.pixels(renderer)?;
            write_pixels(&copy.buffer, Rectangle::from_size(size), pixels, false)
                .map_err(|_| "capture buffer constraints mismatch")?;
            Ok(())
        })();
        if result.is_ok() {
            let timestamp = monotonic();
            let secs = timestamp.as_secs();
            copy.frame.transform(wl_output::Transform::Normal);
            copy.frame.damage(0, 0, size.w, size.h);
            copy.frame.presentation_time(
                (secs >> 32) as u32,
                secs as u32,
                timestamp.subsec_nanos(),
            );
            copy.frame.ready();
            state.image_capture.frames += 1;
            if let Source::Window(surface) = &copy.session.source {
                if let (Some(w), Some(output)) =
                    (window(state, surface), state.space.outputs().next())
                {
                    w.send_frame(
                        output,
                        state.start_time.elapsed(),
                        Some(std::time::Duration::ZERO),
                        |_, _| Some(output.clone()),
                    );
                }
            }
        } else {
            copy.frame
                .failed(ext_image_copy_capture_frame_v1::FailureReason::BufferConstraints);
            state.image_capture.failures += 1;
        }
    }
    let _ = renderer.cleanup_texture_cache();
    state.screencast.mark_copy();
}
fn monotonic() -> std::time::Duration {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}
macro_rules! global {
    ($ty:ty) => {
        impl GlobalDispatch<$ty, Policy> for Ember {
            fn bind(
                _: &mut Self,
                _: &DisplayHandle,
                _: &Client,
                id: New<$ty>,
                _: &Policy,
                init: &mut DataInit<'_, Self>,
            ) {
                init.init(id, ());
            }
            fn can_view(client: Client, policy: &Policy) -> bool {
                visible(&client, policy)
            }
        }
    };
}
global!(ExtImageCopyCaptureManagerV1);
global!(ExtOutputImageCaptureSourceManagerV1);
global!(ExtForeignToplevelImageCaptureSourceManagerV1);
impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for Ember {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &ExtOutputImageCaptureSourceManagerV1,
        request: ext_output_image_capture_source_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_output_image_capture_source_manager_v1::Request::CreateSource {
            source,
            output,
        } = request
        {
            let source_data = if permitted(state, client) {
                Output::from_resource(&output)
                    .map(Source::Output)
                    .unwrap_or_default()
            } else {
                Source::Unavailable
            };
            init.init(source, source_data);
        }
    }
}
impl Dispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for Ember {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &ExtForeignToplevelImageCaptureSourceManagerV1,
        request: ext_foreign_toplevel_image_capture_source_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_foreign_toplevel_image_capture_source_manager_v1::Request::CreateSource {
            source,
            toplevel_handle,
        } = request
        {
            let handle = ForeignToplevelHandle::from_resource(&toplevel_handle);
            let source_data = if permitted(state, client) {
                handle
                    .filter(|h| !h.is_closed())
                    .and_then(|h| {
                        state.windows.iter().find(|m| {
                            m.foreign
                                .as_ref()
                                .is_some_and(|f| f.identifier() == h.identifier())
                        })
                    })
                    .and_then(|m| crate::wm::surface(&m.window))
                    .map(Source::Window)
                    .unwrap_or_default()
            } else {
                Source::Unavailable
            };
            init.init(source, source_data);
        }
    }
}
impl Dispatch<ExtImageCaptureSourceV1, Source> for Ember {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtImageCaptureSourceV1,
        _:smithay::reexports::wayland_protocols::ext::image_capture_source::v1::server::ext_image_capture_source_v1::Request,
        _: &Source,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}
impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for Ember {
    fn request(
        state: &mut Self,
        client: &Client,
        manager: &ExtImageCopyCaptureManagerV1,
        request: ext_image_copy_capture_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_image_copy_capture_manager_v1::Request::CreateSession {
                session,
                source,
                options,
            } => {
                let WEnum::Value(options) = options else {
                    manager.post_error(
                        ext_image_copy_capture_manager_v1::Error::InvalidOption,
                        "invalid capture options",
                    );
                    return;
                };
                let source = if permitted(state, client) {
                    source.data::<Source>().cloned().unwrap_or_default()
                } else {
                    Source::Unavailable
                };
                new_session(
                    state,
                    session,
                    source,
                    options.contains(ext_image_copy_capture_manager_v1::Options::PaintCursors),
                    init,
                );
            }
            ext_image_copy_capture_manager_v1::Request::CreatePointerCursorSession {
                session,
                ..
            } => {
                init.init(session, AtomicBool::new(false));
            }
            _ => {}
        }
    }
}
impl Dispatch<ExtImageCopyCaptureSessionV1, Arc<SessionData>> for Ember {
    fn request(
        _: &mut Self,
        _: &Client,
        session: &ExtImageCopyCaptureSessionV1,
        request: ext_image_copy_capture_session_v1::Request,
        data: &Arc<SessionData>,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_image_copy_capture_session_v1::Request::CreateFrame { frame } = request {
            let mut inner = data.inner.lock().unwrap();
            if inner.frame.as_ref().is_some_and(|f| f.upgrade().is_ok()) {
                session.post_error(
                    ext_image_copy_capture_session_v1::Error::DuplicateFrame,
                    "destroy previous capture frame first",
                );
                return;
            }
            let frame = init.init(
                frame,
                FrameData {
                    session: data.clone(),
                    inner: Mutex::new(FrameInner::default()),
                },
            );
            inner.frame = Some(frame.downgrade());
        }
    }
}
impl Dispatch<ExtImageCopyCaptureFrameV1, FrameData> for Ember {
    fn request(
        state: &mut Self,
        client: &Client,
        frame: &ExtImageCopyCaptureFrameV1,
        request: ext_image_copy_capture_frame_v1::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let mut inner = data.inner.lock().unwrap();
        if matches!(request, ext_image_copy_capture_frame_v1::Request::Destroy) {
            return;
        }
        if inner.used {
            frame.post_error(
                ext_image_copy_capture_frame_v1::Error::AlreadyCaptured,
                "frame already captured",
            );
            return;
        }
        match request {
            ext_image_copy_capture_frame_v1::Request::AttachBuffer { buffer } => {
                inner.buffer = Some(buffer)
            }
            ext_image_copy_capture_frame_v1::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => {
                if x < 0 || y < 0 || width <= 0 || height <= 0 {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::InvalidBufferDamage,
                        "invalid damage rectangle",
                    );
                }
            }
            ext_image_copy_capture_frame_v1::Request::Capture => {
                inner.used = true;
                let Some(buffer) = inner.buffer.take() else {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::NoBuffer,
                        "attach a buffer before capture",
                    );
                    return;
                };
                if !permitted(state, client) || data.session.inner.lock().unwrap().stopped {
                    frame.failed(ext_image_copy_capture_frame_v1::FailureReason::Stopped);
                    return;
                }
                if state.image_capture.pending.len() >= 8 {
                    frame.failed(ext_image_copy_capture_frame_v1::FailureReason::Unknown);
                    return;
                }
                state.image_capture.pending.push(Copy {
                    frame: frame.clone(),
                    buffer,
                    session: data.session.clone(),
                });
                state.dirty = true;
            }
            _ => {}
        }
    }
}
impl Dispatch<ExtImageCopyCaptureCursorSessionV1, AtomicBool> for Ember {
    fn request(
        state: &mut Self,
        _: &Client,
        session: &ExtImageCopyCaptureCursorSessionV1,
        request: ext_image_copy_capture_cursor_session_v1::Request,
        data: &AtomicBool,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ext_image_copy_capture_cursor_session_v1::Request::GetCaptureSession {
            session: id,
        } = request
        {
            if data.swap(true, Ordering::Relaxed) {
                session.post_error(
                    ext_image_copy_capture_cursor_session_v1::Error::DuplicateSession,
                    "cursor session already retrieved",
                );
                return;
            }
            // Separate cursor streams are unavailable; embedded/hidden cursors are supported.
            new_session(state, id, Source::Unavailable, false, init);
        }
    }
}
impl ForeignToplevelListHandler for Ember {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevel_list
    }
}
smithay::delegate_foreign_toplevel_list!(Ember);

pub fn refresh_toplevels(state: &mut Ember) {
    for m in &mut state.windows {
        let Some(surface) = crate::wm::surface(&m.window) else {
            continue;
        };
        let mapped = m.initialized
            && m.window.alive()
            && with_states(&surface, |s| {
                s.data_map
                    .get::<RendererSurfaceStateUserData>()
                    .is_some_and(|d| d.lock().unwrap().buffer().is_some())
            });
        if !mapped {
            if let Some(f) = m.foreign.take() {
                f.send_closed();
            }
            continue;
        }
        let (app_id, title) = crate::wm::metadata(&m.window);
        if let Some(f) = &m.foreign {
            if f.title() != title || f.app_id() != app_id {
                f.send_title(&title);
                f.send_app_id(&app_id);
                f.send_done();
            }
        } else {
            m.foreign = Some(
                state
                    .foreign_toplevel_list
                    .new_toplevel::<Ember>(title, app_id),
            );
        }
    }
}
