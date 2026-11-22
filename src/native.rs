//! Native DRM/KMS + GBM/EGL output and libseat/libinput session backend.
//! Resources stay owned by the backend; calloop callbacks enqueue events for it.
use crate::{config::color, render::Scene, CalloopData, Ember};
use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime, DrmNode,
            GbmBufferedSurface,
        },
        egl::{EGLContext, EGLDisplay},
        input::KeyState,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{damage::OutputDamageTracker, gles::GlesRenderer, Bind, ImportDma, Renderer},
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::{primary_gpu, UdevBackend},
    },
    desktop::utils::OutputPresentationFeedback,
    input::keyboard::FilterResult,
    output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        drm::control::{connector, crtc, Device as ControlDevice, Mode},
        input::Libinput,
        rustix::fs::OFlags,
    },
    utils::{Clock, DeviceFd, Monotonic, Transform, SERIAL_COUNTER},
    wayland::{
        dmabuf::DmabufFeedbackBuilder,
        presentation::{PresentationState, Refresh},
    },
};
use std::{error::Error, path::PathBuf, time::Duration};

type OutputChoice = (connector::Info, crtc::Handle, Mode);
type Swapchain = GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, OutputPresentationFeedback>;
#[derive(Debug)]
pub enum NativeEvent {
    Session(SessionEvent),
    Drm(DrmEvent, Option<DrmEventMetadata>),
    Rescan,
}
struct Scanout {
    swapchain: Swapchain,
    output: Output,
    damage: OutputDamageTracker,
    connector: connector::Handle,
    mode: Mode,
    in_flight: bool,
}
// Drop scanout and GPU resources before the libseat notifier in the outer event loop.
pub struct Native {
    scanout: Option<Scanout>,
    _presentation: PresentationState,
    renderer: GlesRenderer,
    gbm: GbmDevice<DrmDeviceFd>,
    device: DrmDevice,
    session: LibSeatSession,
    input: Libinput,
    scene: Scene,
    preferred_connector: Option<String>,
    requested_refresh_hz: f64,
}
impl Native {
    pub fn init(
        event_loop: &mut EventLoop<CalloopData>,
        data: &mut CalloopData,
    ) -> Result<Self, Box<dyn Error>> {
        // Refuse to take an existing graphical session's devices accidentally.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
            return Err(
                "native mode requires a fresh login/TTY session; use --nested inside a desktop"
                    .into(),
            );
        }
        data.state.backend_name = "native";
        let (mut session, notifier) = LibSeatSession::new().map_err(|e| {
            format!("could not acquire seat (start from SDDM or an active local TTY): {e}")
        })?;
        let seat = session.seat();
        let udev = UdevBackend::new(&seat)?;
        let path = if let Some(path) = std::env::var_os("EMBER_DRM_DEVICE") {
            PathBuf::from(path)
        } else {
            primary_gpu(&seat)?
                .or_else(|| udev.device_list().next().map(|(_, p)| p.to_path_buf()))
                .ok_or("no DRM graphics device on this seat")?
        };
        let fd = session.open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )?;
        let fd = DrmDeviceFd::new(DeviceFd::from(fd));
        let (device, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
        let gbm = GbmDevice::new(fd.clone())?;
        // SAFETY: GBM owns the display connection and outlives the renderer.
        let display = unsafe { EGLDisplay::new(gbm.clone())? };
        let context = EGLContext::new(&display)?;
        // SAFETY: the context is newly created and used only on this event-loop thread.
        let renderer = unsafe { GlesRenderer::new(context)? };
        let node = DrmNode::from_file(&fd)?;
        let feedback =
            DmabufFeedbackBuilder::new(node.dev_id(), renderer.dmabuf_formats()).build()?;
        data.state
            .dmabuf_state
            .create_global_with_default_feedback::<Ember>(&data.display_handle, &feedback);

        let mut input = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
            session.clone().into(),
        );
        input
            .udev_assign_seat(&seat)
            .map_err(|_| "libinput could not assign seat")?;
        let input_backend = LibinputInputBackend::new(input.clone());
        event_loop
            .handle()
            .insert_source(input_backend, |event, _, data| {
                if data.state.session_active {
                    data.state.process_input_event(event);
                }
            })?;
        event_loop
            .handle()
            .insert_source(notifier, |event, _, data| {
                data.state.session_active = matches!(event, SessionEvent::ActivateSession);
                data.state.native_events.push(NativeEvent::Session(event));
            })?;
        event_loop
            .handle()
            .insert_source(drm_notifier, |event, metadata, data| {
                data.state
                    .native_events
                    .push(NativeEvent::Drm(event, *metadata));
            })?;
        let device_id = device.device_id();
        event_loop
            .handle()
            .insert_source(udev, move |event, _, data| {
                use smithay::backend::udev::UdevEvent;
                let id = match event {
                    UdevEvent::Added { device_id, .. }
                    | UdevEvent::Changed { device_id }
                    | UdevEvent::Removed { device_id } => device_id,
                };
                if id == device_id {
                    data.state.native_events.push(NativeEvent::Rescan);
                }
            })?;
        let mut backend = Self {
            scanout: None,
            // Only native advertises hardware presentation feedback. The nested
            // backend cannot know when its host compositor actually presents.
            _presentation: PresentationState::new::<Ember>(
                &data.display_handle,
                libc::CLOCK_MONOTONIC as u32,
            ),
            renderer,
            gbm,
            device,
            session,
            input,
            scene: Scene::new(),
            preferred_connector: std::env::var("EMBER_DRM_CONNECTOR").ok(),
            requested_refresh_hz: data.state.config.display.refresh_hz,
        };
        backend.rescan(data)?;
        if backend.scanout.is_none() {
            return Err("no connected display with a usable mode/CRTC".into());
        }
        data.state.session_active = backend.session.is_active();
        tracing::info!(?path, %seat, "native DRM backend initialized");
        Ok(backend)
    }
    fn select_output(&self, requested_hz: f64) -> Result<Option<OutputChoice>, Box<dyn Error>> {
        let resources = self.device.resource_handles()?;
        for handle in resources.connectors() {
            let info = self.device.get_connector(*handle, true)?;
            if info.state() != connector::State::Connected || info.modes().is_empty() {
                continue;
            }
            let name = format!("{:?}-{}", info.interface(), info.interface_id());
            if self
                .preferred_connector
                .as_ref()
                .is_some_and(|p| p != &name)
            {
                continue;
            }
            let mode = crate::display::choose_mode(info.modes(), requested_hz).unwrap();
            let mut chosen = None;
            for encoder in info.encoders() {
                let encoder = self.device.get_encoder(*encoder)?;
                chosen = encoder.crtc().or_else(|| {
                    resources
                        .filter_crtcs(encoder.possible_crtcs())
                        .into_iter()
                        .next()
                });
                if chosen.is_some() {
                    break;
                }
            }
            if let Some(crtc) = chosen {
                return Ok(Some((info, crtc, mode)));
            }
        }
        Ok(None)
    }
    fn rescan(&mut self, data: &mut CalloopData) -> Result<(), Box<dyn Error>> {
        if !self.device.is_active() {
            return Ok(());
        }
        let chosen = self.select_output(data.state.config.display.refresh_hz)?;
        if let (Some((info, _, mode)), Some(current)) = (&chosen, &self.scanout) {
            if info.handle() == current.connector && *mode == current.mode {
                return Ok(());
            }
        }
        if let Some(old) = self.scanout.take() {
            data.state.space.unmap_output(&old.output);
            data.display_handle.remove_global::<Ember>(
                old.output
                    .user_data()
                    .get::<OutputGlobal>()
                    .unwrap()
                    .0
                    .clone(),
            );
            drop(old);
        }
        let Some((info, crtc, mode)) = chosen else {
            tracing::warn!("all supported displays disconnected; waiting for hotplug");
            return Ok(());
        };
        let surface = self.device.create_surface(crtc, mode, &[info.handle()])?;
        let allocator = GbmAllocator::new(
            self.gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        );
        let swapchain = GbmBufferedSurface::new(
            surface,
            allocator,
            &[Fourcc::Abgr8888, Fourcc::Argb8888],
            self.renderer.dmabuf_formats(),
        )?;
        let name = format!("{:?}-{}", info.interface(), info.interface_id());
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: info
                    .size()
                    .map(|(w, h)| (w as i32, h as i32))
                    .unwrap_or((0, 0))
                    .into(),
                subpixel: Subpixel::Unknown,
                make: "Ember".into(),
                model: name,
            },
        );
        let global = output.create_global::<Ember>(&data.display_handle);
        output
            .user_data()
            .insert_if_missing(|| OutputGlobal(global));
        let output_mode = OutputMode {
            size: (mode.size().0 as i32, mode.size().1 as i32).into(),
            refresh: crate::display::refresh_millihz(&mode),
        };
        output.change_current_state(
            Some(output_mode),
            Some(Transform::Normal),
            None,
            Some((0, 0).into()),
        );
        output.set_preferred(output_mode);
        data.state.space.map_output(&output, (0, 0));
        data.state.relayout();
        self.scanout = Some(Scanout {
            damage: OutputDamageTracker::from_output(&output),
            output,
            swapchain,
            connector: info.handle(),
            mode,
            in_flight: false,
        });
        self.requested_refresh_hz = data.state.config.display.refresh_hz;
        let actual_hz = f64::from(crate::display::refresh_millihz(&mode)) / 1000.0;
        if self.requested_refresh_hz > 0.0 && (actual_hz - self.requested_refresh_hz).abs() > 1.0 {
            tracing::warn!(
                requested_hz = self.requested_refresh_hz,
                actual_hz,
                "requested rate unavailable at preferred resolution; using nearest advertised mode"
            );
        }
        tracing::info!(?mode, actual_hz, "native display configured");
        Ok(())
    }
    pub fn process_events(&mut self, data: &mut CalloopData) -> Result<(), Box<dyn Error>> {
        for event in std::mem::take(&mut data.state.native_events) {
            match event {
                NativeEvent::Session(SessionEvent::PauseSession) => {
                    self.input.suspend();
                    self.device.pause();
                    // Clear all modifiers so switching VTs cannot leave Ctrl/Alt stuck.
                    let keyboard = data.state.seat.get_keyboard().unwrap();
                    for code in keyboard.pressed_keys() {
                        keyboard.input::<(), _>(
                            &mut data.state,
                            code,
                            KeyState::Released,
                            SERIAL_COUNTER.next_serial(),
                            0,
                            |_, _, _| FilterResult::Forward,
                        );
                    }
                    data.state.suppressed_keys.clear();
                    data.state.suppressed_buttons.clear();
                    data.state.draw = None;
                    data.state.drag = None;
                    tracing::info!("native session paused");
                }
                NativeEvent::Session(SessionEvent::ActivateSession) => {
                    self.device.activate(true)?;
                    self.input.resume().map_err(|_| "libinput resume failed")?;
                    // A pending flip may have been canceled while the seat was inactive.
                    if let Some(scanout) = &mut self.scanout {
                        let _ = scanout.swapchain.frame_submitted();
                        scanout.in_flight = false;
                        scanout.swapchain.reset_buffer_ages();
                        scanout.damage = OutputDamageTracker::from_output(&scanout.output);
                    }
                    self.rescan(data)?;
                    data.state.dirty = true;
                    tracing::info!("native session resumed");
                }
                NativeEvent::Drm(DrmEvent::VBlank(crtc), metadata) => {
                    if let Some(scanout) = &mut self.scanout {
                        if scanout.swapchain.crtc() == crtc {
                            if let Some(mut feedback) = scanout.swapchain.frame_submitted()? {
                                use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind;
                                let (time, sequence, flags) = match metadata {
                                    Some(DrmEventMetadata {
                                        time: DrmEventTime::Monotonic(time),
                                        sequence,
                                    }) => (
                                        time.into(),
                                        u64::from(sequence),
                                        Kind::Vsync | Kind::HwClock | Kind::HwCompletion,
                                    ),
                                    other => (
                                        Clock::<Monotonic>::new().now(),
                                        other.map_or(0, |m| u64::from(m.sequence)),
                                        Kind::Vsync | Kind::HwCompletion,
                                    ),
                                };
                                feedback.presented(
                                    time,
                                    Refresh::fixed(crate::display::frame_interval(
                                        crate::display::refresh_millihz(&scanout.mode),
                                    )),
                                    sequence,
                                    flags,
                                );
                            }
                            scanout.in_flight = false;
                        }
                    }
                }
                NativeEvent::Drm(DrmEvent::Error(error), _) => {
                    tracing::warn!(%error,"DRM event error")
                }
                NativeEvent::Rescan => self.rescan(data)?,
            }
        }
        if self.requested_refresh_hz != data.state.config.display.refresh_hz
            && self.can_render(&data.state)
        {
            self.rescan(data)?;
            self.requested_refresh_hz = data.state.config.display.refresh_hz;
        }
        if let Some(vt) = data.state.vt_request.take() {
            if let Err(error) = self.session.change_vt(vt) {
                tracing::warn!(%error,vt,"VT switch failed");
            }
        }
        Ok(())
    }
    pub fn can_render(&self, state: &Ember) -> bool {
        state.session_active && self.scanout.as_ref().is_some_and(|s| !s.in_flight)
    }
    pub fn import_buffers(&mut self, data: &mut CalloopData) {
        if !data.state.session_active {
            return;
        }
        for (buffer, notifier) in data.state.dmabuf_pending.drain(..) {
            if self.renderer.import_dmabuf(&buffer, None).is_ok() {
                let _ = notifier.successful::<Ember>();
            } else {
                notifier.failed();
            }
        }
    }
    pub fn render(&mut self, data: &mut CalloopData) -> Result<(), Box<dyn Error>> {
        crate::image_capture::render(&mut data.state, &mut self.scene, &mut self.renderer);
        let capturing = data.state.screencast.pending();
        let no_cursor =
            crate::screencast::without_cursor(&mut self.scene, &mut self.renderer, &data.state);
        let Some(scanout) = &mut self.scanout else {
            return Ok(());
        };
        let (mut buffer, age) = scanout.swapchain.next_buffer()?;
        let mut framebuffer = self.renderer.bind(&mut buffer)?;
        let elements = self.scene.elements(&data.state, &mut self.renderer);
        let force = data.state.screenshot.is_some()
            || data.state.clipboard_capture.is_some()
            || data.state.screencast.pending();
        let result = scanout.damage.render_output(
            &mut self.renderer,
            &mut framebuffer,
            if force { 0 } else { age as usize },
            &elements,
            color(&data.state.config.theme.background).unwrap(),
        )?;
        let sync = result.sync;
        let damage = result.damage.cloned();
        let render_states = result.states;
        if let Some(path) = data.state.screenshot.take() {
            let size = data.state.output_size();
            if let Err(error) = crate::winit::capture(
                &mut self.renderer,
                &framebuffer,
                size.w,
                size.h,
                crate::winit::RowOrder::TopFirst,
                &path,
            ) {
                tracing::warn!(%error,"native screenshot failed");
            }
        }
        let size = data.state.output_size();
        let copied = crate::winit::capture_clipboard(
            &mut self.renderer,
            &framebuffer,
            size.w,
            size.h,
            crate::winit::RowOrder::TopFirst,
            &mut data.state,
        );
        if data.state.screencast.pending() {
            let mapping = if data.state.screencast.needs_cursor() {
                crate::screencast::Readback::capture(
                    &mut self.renderer,
                    &framebuffer,
                    size.w,
                    size.h,
                    false,
                )
                .ok()
            } else {
                None
            };
            let pixels = mapping
                .as_ref()
                .and_then(|m| m.pixels(&mut self.renderer).ok());
            let without_cursor = no_cursor
                .as_ref()
                .and_then(|m| m.pixels(&mut self.renderer).ok());
            data.state.screencast.complete(
                pixels,
                without_cursor,
                size,
                data.state.start_time.elapsed(),
            );
        }
        drop(no_cursor);
        if capturing {
            let _ = self.renderer.cleanup_texture_cache();
        }
        drop(framebuffer);
        let mut feedback = OutputPresentationFeedback::new(&scanout.output);
        for window in data.state.space.elements() {
            window.take_presentation_feedback(&mut feedback,
                |surface, _| render_states.element_was_presented(
                    smithay::backend::renderer::element::Id::from_wayland_resource(surface))
                    .then(|| scanout.output.clone()),
                |_, _| smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind::empty());
        }
        if let Some(damage) = damage {
            scanout
                .swapchain
                .queue_buffer(Some(sync), Some(damage), feedback)?;
            scanout.in_flight = true;
            data.state.frames_submitted += 1;
        } else {
            // No new scanout was submitted: do not invent a presentation time.
            feedback.discarded();
        }
        for window in data.state.space.elements() {
            window.send_frame(
                &scanout.output,
                data.state.start_time.elapsed(),
                Some(Duration::ZERO),
                |_, _| Some(scanout.output.clone()),
            );
        }
        data.state.dirty = copied;
        Ok(())
    }
}
struct OutputGlobal(smithay::reexports::wayland_server::backend::GlobalId);
