//! wlr-screencopy v3 + native portal consent. Buffers exist only for pending copies.
use crate::{
    config::{color, Config},
    layout::Rect,
    panels, ui, Ember,
};
use serde::{Deserialize, Serialize};
use smithay::{
    output::Output,
    reexports::{
        wayland_protocols_wlr::screencopy::v1::server::{
            zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
            zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
        },
        wayland_server::{
            protocol::{wl_buffer::WlBuffer, wl_shm},
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        },
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::shm::with_buffer_contents_mut,
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// The GPU mapping is copied straight into client SHM; no full-size CPU image clone.
pub struct Readback {
    mapping: smithay::backend::renderer::gles::GlesMapping,
    width: usize,
    height: usize,
    bottom_first: bool,
}
#[derive(Clone, Copy)]
pub struct Pixels<'a> {
    bytes: &'a [u8],
    width: usize,
    height: usize,
    bottom_first: bool,
}
impl Readback {
    pub fn capture(
        renderer: &mut smithay::backend::renderer::gles::GlesRenderer,
        target: &<smithay::backend::renderer::gles::GlesRenderer as smithay::backend::renderer::RendererSuper>::Framebuffer<'_>,
        width: i32,
        height: i32,
        bottom_first: bool,
    ) -> Result<Self, smithay::backend::renderer::gles::GlesError> {
        use smithay::backend::{allocator::Fourcc, renderer::ExportMem};
        let mapping = renderer.copy_framebuffer(
            target,
            Rectangle::from_size((width, height).into()),
            Fourcc::Abgr8888,
        )?;
        Ok(Self {
            mapping,
            width: width as usize,
            height: height as usize,
            bottom_first,
        })
    }
    pub fn pixels<'a>(
        &'a self,
        renderer: &mut smithay::backend::renderer::gles::GlesRenderer,
    ) -> Result<Pixels<'a>, smithay::backend::renderer::gles::GlesError> {
        use smithay::backend::renderer::ExportMem;
        Ok(Pixels {
            bytes: renderer.map_texture(&self.mapping)?,
            width: self.width,
            height: self.height,
            bottom_first: self.bottom_first,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub allow_direct_capture: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_direct_capture: false,
        }
    }
}
pub struct FrameData {
    rect: Rect,
    output: Option<Output>,
    cursor: bool,
    used: AtomicBool,
}
struct Copy {
    frame: ZwlrScreencopyFrameV1,
    buffer: WlBuffer,
    rect: Rect,
    output: Output,
    cursor: bool,
    damage: bool,
}
#[derive(Clone)]
pub struct Choice {
    pub label: String,
    pub value: String,
}
struct Prompt {
    token: u64,
    choices: Vec<Choice>,
    selected: usize,
    deadline: Instant,
    decision: Option<bool>,
    allow_selected: bool,
}
#[derive(Default)]
pub struct Runtime {
    pub(crate) direct_capture: std::sync::Arc<AtomicBool>,
    pending: Vec<Copy>,
    prompt: Option<Prompt>,
    pub view: Option<ui::View>,
    buttons: Vec<(Rect, bool)>,
    rows: Vec<(Rect, usize)>,
    key: Option<(u64, u64, i32, i32, bool, usize)>,
    frames: u64,
    failures: u64,
    last_copy: Option<Instant>,
}
impl Runtime {
    pub fn new(dh: &DisplayHandle, settings: &Settings) -> Self {
        let direct_capture = std::sync::Arc::new(AtomicBool::new(settings.allow_direct_capture));
        dh.create_global::<Ember, ZwlrScreencopyManagerV1, _>(
            3,
            CapturePolicy {
                direct: direct_capture.clone(),
            },
        );
        Self {
            direct_capture,
            ..Default::default()
        }
    }
    pub fn mark_copy(&mut self) {
        self.last_copy = Some(Instant::now());
    }
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn needs_no_cursor(&self) -> bool {
        self.pending.iter().any(|p| !p.cursor)
    }
    pub fn needs_cursor(&self) -> bool {
        self.pending.iter().any(|p| p.cursor)
    }
    pub fn prompt_open(&self) -> bool {
        self.prompt.as_ref().is_some_and(|p| p.decision.is_none())
    }
    pub fn request(&mut self, choices: Vec<Choice>) -> Result<u64, String> {
        if choices.is_empty() {
            return Err("no available sharing sources".into());
        }
        if self.prompt.is_some() {
            return Err("another screen-sharing request is pending".into());
        }
        let token = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.prompt = Some(Prompt {
            token,
            choices,
            selected: 0,
            deadline: Instant::now() + Duration::from_secs(60),
            decision: None,
            allow_selected: false,
        });
        self.key = None;
        Ok(token)
    }
    pub fn decide(&mut self, allow: bool) {
        if let Some(p) = &mut self.prompt {
            p.decision = Some(allow);
        }
        self.view = None;
        self.buttons = Vec::new();
        self.rows = Vec::new();
        self.key = None;
        crate::memory::request_reclaim();
    }
    pub fn select_prompt(&mut self) {
        if let Some(p) = &mut self.prompt {
            p.allow_selected = !p.allow_selected;
            self.key = None;
        }
    }
    pub fn confirm_prompt(&mut self) {
        self.decide(self.prompt.as_ref().is_some_and(|p| p.allow_selected));
    }
    pub fn result(&mut self, token: u64) -> Result<Option<String>, String> {
        let p = self
            .prompt
            .as_ref()
            .filter(|p| p.token == token)
            .ok_or("unknown share request")?;
        match p.decision {
            None => Ok(None),
            Some(allow) => {
                let name = if allow {
                    p.choices[p.selected].value.clone()
                } else {
                    String::new()
                };
                self.prompt = None;
                Ok(Some(name))
            }
        }
    }
    pub fn click(&mut self, pos: Point<f64, Logical>, right: bool) {
        if right
            || self
                .view
                .as_ref()
                .is_none_or(|v| !v.rect.to_f64().contains(pos))
        {
            self.decide(false);
        } else if let Some((_, index)) = self.rows.iter().find(|(r, _)| r.to_f64().contains(pos)) {
            if let Some(p) = &mut self.prompt {
                p.selected = *index;
                self.key = None;
            }
        } else if let Some((_, allow)) = self
            .buttons
            .iter()
            .find(|(rect, _)| rect.to_f64().contains(pos))
        {
            self.decide(*allow);
        }
    }
    pub fn select_source(&mut self, down: bool) {
        if let Some(p) = &mut self.prompt {
            p.selected = if down {
                (p.selected + 1) % p.choices.len()
            } else {
                (p.selected + p.choices.len() - 1) % p.choices.len()
            };
            self.key = None;
        }
    }
    pub fn choose_source(&mut self, value: &str) -> Result<(), String> {
        let p = self.prompt.as_mut().ok_or("no sharing prompt")?;
        p.selected = p
            .choices
            .iter()
            .position(|c| c.value == value)
            .ok_or("unknown sharing source")?;
        self.key = None;
        Ok(())
    }
    pub fn next_wake(&self) -> Option<Duration> {
        let prompt = self
            .prompt
            .as_ref()
            .map(|p| p.deadline.saturating_duration_since(Instant::now()));
        let reclaim = self
            .last_copy
            .map(|t| (t + Duration::from_secs(2)).saturating_duration_since(Instant::now()));
        match (prompt, reclaim) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    pub fn update(
        &mut self,
        config: &Config,
        path: &Path,
        generation: u64,
        size: Size<i32, Logical>,
        active: bool,
    ) -> bool {
        self.direct_capture
            .store(config.screencast.allow_direct_capture, Ordering::Relaxed);
        self.pending
            .retain(|p| p.frame.is_alive() && p.buffer.is_alive());
        if self.pending.is_empty()
            && self
                .last_copy
                .is_some_and(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.last_copy = None;
            crate::memory::request_reclaim();
        }
        if !config.screencast.enabled || !active {
            self.fail_pending();
            if self.prompt_open() {
                self.decide(false);
                return true;
            }
        }
        if self
            .prompt
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.deadline)
        {
            self.decide(false);
            self.prompt = None;
            return true;
        }
        if !self.prompt_open() {
            return false;
        }
        let token = self.prompt.as_ref().unwrap().token;
        let allow_selected = self.prompt.as_ref().unwrap().allow_selected;
        let selected = self.prompt.as_ref().unwrap().selected;
        let key = (token, generation, size.w, size.h, allow_selected, selected);
        if self.key == Some(key) {
            return false;
        }
        self.key = Some(key);
        let Ok(font) = panels::load_font(&config.panels, path) else {
            self.decide(false);
            return true;
        };
        let width = 540.min((size.w - 32).max(1));
        let visible = self.prompt.as_ref().unwrap().choices.len().min(6);
        let height = (200 + visible as i32 * 32).min((size.h - 32).max(1));
        let rect = Rectangle::new(
            ((size.w - width) / 2, (size.h - height) / 2).into(),
            (width, height).into(),
        );
        let fg = color(&config.panels.foreground).unwrap();
        let bg = color(&config.panels.background).unwrap();
        let accent = color(&config.panels.accent).unwrap();
        let border = color(&config.panels.border_color).unwrap();
        let mut buttons = Vec::new();
        let prompt = self.prompt.as_ref().unwrap();
        let first = selected.saturating_sub(visible.saturating_sub(1));
        let choices = prompt.choices.clone();
        let mut rows = Vec::new();
        let buffer = ui::surface(width, height, |pixels| {
            panels::fill(pixels, (width, height), (0, 0, width, height), bg);
            for edge in [
                (0, 0, width, 1),
                (0, height - 1, width, 1),
                (0, 0, 1, height),
                (width - 1, 0, 1, height),
            ] {
                panels::fill(pixels, (width, height), edge, accent);
            }
            ui::text(
                pixels,
                (width, height),
                &font,
                "Choose what to share",
                16.,
                18,
                16,
                accent,
            );
            ui::text(
                pixels,
                (width, height),
                &font,
                "Up / down: select a display or a single window.",
                12.,
                18,
                52,
                fg,
            );
            for (index, choice) in choices.iter().enumerate().skip(first).take(visible) {
                let y = 80 + (index - first) as i32 * 32;
                if index == selected {
                    panels::fill(pixels, (width, height), (16, y, width - 32, 30), border);
                }
                ui::text(
                    pixels,
                    (width, height),
                    &font,
                    &ui::fit(&font, &choice.label, 12., width - 48),
                    12.,
                    24,
                    y + 5,
                    if index == selected { accent } else { fg },
                );
                rows.push((
                    Rectangle::new(
                        (rect.loc.x + 16, rect.loc.y + y).into(),
                        (width - 32, 30).into(),
                    ),
                    index,
                ));
            }
            for (x, w, label, allow) in [
                (18, 100, "Cancel", false),
                (width - 154, 136, "Share selected", true),
            ] {
                let y = height - 73;
                panels::fill(
                    pixels,
                    (width, height),
                    (x, y, w, 32),
                    if allow == allow_selected {
                        accent
                    } else {
                        border
                    },
                );
                ui::text(pixels, (width, height), &font, label, 12., x + 7, y + 6, fg);
                buttons.push((
                    Rectangle::new((rect.loc.x + x, rect.loc.y + y).into(), (w, 32).into()),
                    allow,
                ));
            }
            ui::text(
                pixels,
                (width, height),
                &font,
                "Tab / arrows: choose · Enter: confirm · Esc: cancel",
                11.,
                18,
                height - 26,
                fg,
            );
        });
        self.buttons = buttons;
        self.rows = rows;
        self.view = Some(ui::View::overlay(rect, buffer));
        true
    }
    fn fail_pending(&mut self) {
        for copy in self.pending.drain(..) {
            if copy.frame.is_alive() {
                copy.frame.failed();
                self.failures += 1;
            }
        }
        self.pending = Vec::new();
    }
    pub fn complete(
        &mut self,
        with_cursor: Option<Pixels<'_>>,
        without_cursor: Option<Pixels<'_>>,
        size: Size<i32, Logical>,
        timestamp: Duration,
    ) {
        self.last_copy = Some(Instant::now());
        for copy in self.pending.drain(..) {
            if !copy.frame.is_alive() {
                continue;
            }
            let image = if copy.cursor {
                with_cursor
            } else {
                without_cursor
            };
            let okay = copy
                .output
                .current_mode()
                .is_some_and(|m| m.size.w == size.w && m.size.h == size.h)
                && image.is_some_and(|image| {
                    write_pixels(
                        &copy.buffer,
                        copy.rect,
                        image,
                        copy.output.current_transform() == smithay::utils::Transform::Flipped180,
                    )
                    .is_ok()
                });
            if okay {
                copy.frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
                if copy.damage && copy.frame.version() >= 2 {
                    copy.frame
                        .damage(0, 0, copy.rect.size.w as u32, copy.rect.size.h as u32);
                }
                let seconds = timestamp.as_secs();
                copy.frame.ready(
                    (seconds >> 32) as u32,
                    seconds as u32,
                    timestamp.subsec_nanos(),
                );
                self.frames += 1;
            } else {
                copy.frame.failed();
                self.failures += 1;
            }
        }
        self.pending = Vec::new();
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"pending_frames":self.pending.len(),"frames_copied":self.frames,"failures":self.failures,"prompt_open":self.prompt_open(),"prompt_token":self.prompt.as_ref().map(|p|p.token),"sources":self.prompt.as_ref().map(|p|p.choices.iter().map(|c|serde_json::json!({"label":c.label,"value":c.value})).collect::<Vec<_>>()),"selected_source":self.prompt.as_ref().map(|p|&p.choices[p.selected].value),"view":self.view.as_ref().map(|v|serde_json::json!({"x":v.rect.loc.x,"y":v.rect.loc.y,"width":v.rect.size.w,"height":v.rect.size.h})),"buffer_bytes":self.view.as_ref().map_or(0,|v|v.rect.size.w as u64*v.rect.size.h as u64*4)})
    }
}
pub(crate) fn write_pixels(
    buffer: &WlBuffer,
    rect: Rect,
    image: Pixels<'_>,
    flip_output: bool,
) -> Result<(), ()> {
    if rect.loc.x < 0
        || rect.loc.y < 0
        || rect
            .loc
            .x
            .checked_add(rect.size.w)
            .is_none_or(|x| x > image.width as i32)
        || rect
            .loc
            .y
            .checked_add(rect.size.h)
            .is_none_or(|y| y > image.height as i32)
    {
        return Err(());
    }
    with_buffer_contents_mut(buffer, |ptr, len, data| {
        let bytes = (rect.size.w as usize)
            .checked_mul(rect.size.h as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(())?;
        if data.width != rect.size.w
            || data.height != rect.size.h
            || data.stride != rect.size.w * 4
            || data.format != wl_shm::Format::Argb8888
            || data.offset < 0
            || (data.offset as usize)
                .checked_add(bytes)
                .is_none_or(|end| end > len)
        {
            return Err(());
        }
        // Smithay guards the mapping against SIGBUS; all offsets and lengths were checked.
        let target =
            unsafe { std::slice::from_raw_parts_mut(ptr.add(data.offset as usize), bytes) };
        for y in 0..rect.size.h as usize {
            let target_y = y;
            let y = if flip_output {
                rect.size.h as usize - 1 - y
            } else {
                y
            };
            let logical_y = rect.loc.y as usize + y;
            let source_y = if image.bottom_first {
                image.height - 1 - logical_y
            } else {
                logical_y
            };
            let start = (source_y * image.width + rect.loc.x as usize) * 4;
            let row = &image.bytes[start..start + rect.size.w as usize * 4];
            for (src, dst) in row.as_chunks::<4>().0.iter().zip(
                target[target_y * rect.size.w as usize * 4
                    ..(target_y + 1) * rect.size.w as usize * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut(),
            ) {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
            }
        }
        Ok(())
    })
    .map_err(|_| ())?
}
struct CapturePolicy {
    direct: std::sync::Arc<AtomicBool>,
}
pub fn trusted_peer(stream: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // Read credentials before insert_client. The global visibility callback runs under
    // the Wayland backend lock and must never re-enter DisplayHandle to query them.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut len,
        )
    } != 0
        || len as usize != std::mem::size_of::<libc::ucred>()
        || credentials.uid != unsafe { libc::getuid() }
    {
        return false;
    }
    let pid = credentials.pid;
    if pid <= 0 {
        return false;
    }
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .is_some_and(|p| {
            [
                "/usr/libexec/xdg-desktop-portal-wlr",
                "/usr/lib/xdg-desktop-portal-wlr",
                "/usr/bin/xdg-desktop-portal-wlr",
            ]
            .iter()
            .any(|allowed| p == Path::new(allowed))
        })
}
pub(crate) fn trusted_portal(client: &Client) -> bool {
    client
        .get_data::<crate::state::ClientState>()
        .is_some_and(|data| data.portal_capture)
}
impl GlobalDispatch<ZwlrScreencopyManagerV1, CapturePolicy> for Ember {
    fn can_view(client: Client, policy: &CapturePolicy) -> bool {
        policy.direct.load(Ordering::Relaxed) || trusted_portal(&client)
    }
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _data: &CapturePolicy,
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}
impl Dispatch<ZwlrScreencopyManagerV1, ()> for Ember {
    fn request(
        state: &mut Self,
        client: &Client,
        _manager: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        let (frame, cursor, resource, region) = match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => (frame, overlay_cursor != 0, output, None),
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => (
                frame,
                overlay_cursor != 0,
                output,
                Some(Rectangle::new((x, y).into(), (width, height).into())),
            ),
            _ => return,
        };
        let output = Output::from_resource(&resource)
            .filter(|o| state.space.outputs().any(|current| current == o));
        let size = state.output_size();
        let full = Rectangle::from_size(size);
        let rect = region.and_then(|r| r.intersection(full)).unwrap_or(full);
        let valid = state.config.screencast.enabled
            && (state.config.screencast.allow_direct_capture || trusted_portal(client))
            && state.session_active
            && output.is_some()
            && region
                .is_none_or(|r| r.size.w > 0 && r.size.h > 0 && r.intersection(full).is_some());
        let frame = init.init(
            frame,
            FrameData {
                rect,
                output,
                cursor,
                used: AtomicBool::new(!valid),
            },
        );
        if !valid {
            frame.failed();
            return;
        }
        frame.buffer(
            wl_shm::Format::Argb8888,
            rect.size.w as u32,
            rect.size.h as u32,
            rect.size.w as u32 * 4,
        );
        if frame.version() >= 3 {
            frame.buffer_done();
        }
    }
}
impl Dispatch<ZwlrScreencopyFrameV1, FrameData> for Ember {
    fn request(
        state: &mut Self,
        _client: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &FrameData,
        _dh: &DisplayHandle,
        _init: &mut DataInit<'_, Self>,
    ) {
        let (buffer, damage) = match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => (buffer, false),
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => (buffer, true),
            _ => return,
        };
        if data.used.swap(true, Ordering::Relaxed) {
            frame.post_error(
                zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                "frame has already been copied",
            );
            return;
        }
        if !state.config.screencast.enabled
            || !state.session_active
            || state.screencast.pending.len() >= 8
            || data.output.is_none()
        {
            frame.failed();
            return;
        }
        state.screencast.pending.push(Copy {
            frame: frame.clone(),
            buffer,
            rect: data.rect,
            output: data.output.clone().unwrap(),
            cursor: data.cursor,
            damage,
        });
        state.dirty = true;
    }
}

/// Render a cursor-free capture into a temporary GPU target, released before returning.
pub fn without_cursor(
    scene: &mut crate::render::Scene,
    renderer: &mut smithay::backend::renderer::gles::GlesRenderer,
    state: &Ember,
) -> Option<Readback> {
    use smithay::{
        backend::{
            allocator::Fourcc,
            renderer::{damage::OutputDamageTracker, gles::GlesRenderbuffer, Bind, Offscreen},
        },
        utils::Transform,
    };
    if !state.screencast.needs_no_cursor() {
        return None;
    }
    let size = state.output_size();
    let elements = scene.elements_with_cursor(state, renderer, false);
    let result = (|| -> Result<_, Box<dyn std::error::Error>> {
        let mut target: GlesRenderbuffer =
            renderer.create_buffer(Fourcc::Abgr8888, (size.w, size.h).into())?;
        let mut framebuffer = renderer.bind(&mut target)?;
        let mut damage = OutputDamageTracker::new((size.w, size.h), 1., Transform::Normal);
        damage.render_output(
            renderer,
            &mut framebuffer,
            0,
            &elements,
            color(&state.config.theme.background).unwrap(),
        )?;
        Ok(Readback::capture(
            renderer,
            &framebuffer,
            size.w,
            size.h,
            false,
        )?)
    })();
    result
        .map_err(|error| tracing::warn!(%error,"cursor-free capture failed"))
        .ok()
}
