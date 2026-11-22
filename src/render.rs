use crate::{
    config::{color, Config},
    layout::{self, Rect},
    Ember,
};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements,
                solid::SolidColorRenderElement,
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                texture::{TextureBuffer, TextureRenderElement},
                AsRenderElements, Id, Kind,
            },
            gles::{GlesRenderer, GlesTexture},
            utils::CommitCounter,
        },
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

render_elements! {
    pub SceneElement<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Solid=SolidColorRenderElement,
    Memory=MemoryRenderBufferRenderElement<GlesRenderer>,
    Texture=TextureRenderElement<GlesTexture>,
}

pub fn validate_wallpaper(config: &Config, path: &Path) -> Result<(), String> {
    if let Some(path) = config.wallpaper_path(path) {
        let (w, h) = image::ImageReader::open(&path)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        if w == 0 || h == 0 || w > 8192 || h > 8192 || u64::from(w) * u64::from(h) > 16_777_216 {
            return Err(
                "wallpaper must be nonempty and at most 16 megapixels / 8192 per side".into(),
            );
        }
    }
    Ok(())
}

pub struct Scene {
    generation: u64,
    size: Size<i32, Logical>,
    wallpaper: Option<TextureBuffer<GlesTexture>>,
    wallpaper_source: Option<(PathBuf, Option<SystemTime>)>,
    wallpaper_mode: String,
    wallpaper_palette: Vec<String>,
    wallpaper_location: Point<f64, Physical>,
    selection_ids: [Id; 4],
    selection_fill: Id,
    cursor: MemoryRenderBuffer,
    crosshair_ids: [Id; 2],
    commit: CommitCounter,
}
impl Scene {
    pub fn new() -> Self {
        Self {
            generation: u64::MAX,
            size: (0, 0).into(),
            wallpaper: None,
            wallpaper_source: None,
            wallpaper_mode: String::new(),
            wallpaper_palette: Vec::new(),
            wallpaper_location: (0.0, 0.0).into(),
            selection_ids: std::array::from_fn(|_| Id::new()),
            selection_fill: Id::new(),
            cursor: default_cursor(),
            crosshair_ids: std::array::from_fn(|_| Id::new()),
            commit: CommitCounter::default(),
        }
    }
    pub fn elements(&mut self, state: &Ember, renderer: &mut GlesRenderer) -> Vec<SceneElement> {
        self.elements_with_cursor(state, renderer, true)
    }
    pub fn elements_with_cursor(
        &mut self,
        state: &Ember,
        renderer: &mut GlesRenderer,
        cursor: bool,
    ) -> Vec<SceneElement> {
        let size = state.output_size();
        if self.generation != state.theme_generation || self.size != size {
            let resized = self.size != size;
            self.generation = state.theme_generation;
            self.size = size;
            self.commit.increment();
            let source = state.config.wallpaper_path(&state.config_path).map(|path| {
                let modified = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok());
                (path, modified)
            });
            // Border/keybinding reloads must not decode and upload an unchanged wallpaper.
            if resized
                || source != self.wallpaper_source
                || self.wallpaper_mode != state.config.theme.wallpaper_mode
                || self.wallpaper_palette != state.config.theme.wallpaper_palette
            {
                crate::memory::request_reclaim();
                self.wallpaper_source = source;
                self.wallpaper_mode = state.config.theme.wallpaper_mode.clone();
                self.wallpaper_palette = state.config.theme.wallpaper_palette.clone();
                self.wallpaper = state
                    .config
                    .wallpaper_path(&state.config_path)
                    .and_then(|path| match image::open(path) {
                        Ok(img) => {
                            let (w, h) = (size.w as u32, size.h as u32);
                            let filter = image::imageops::FilterType::Triangle;
                            let mut img = match self.wallpaper_mode.as_str() {
                                "fit" => img.resize(w, h, filter),
                                "stretch" => img.resize_exact(w, h, filter),
                                "center" => {
                                    let (cw, ch) = (img.width().min(w), img.height().min(h));
                                    img.crop_imm(
                                        (img.width() - cw) / 2,
                                        (img.height() - ch) / 2,
                                        cw,
                                        ch,
                                    )
                                }
                                _ => img.resize_to_fill(w, h, filter),
                            }
                            .into_rgba8();
                            apply_wallpaper_palette(&mut img, &self.wallpaper_palette);
                            self.wallpaper_location = (
                                ((w - img.width()) / 2) as f64,
                                ((h - img.height()) / 2) as f64,
                            )
                                .into();
                            let image_size = (img.width() as i32, img.height() as i32);
                            // Keep only the renderer texture after upload. Retaining an extra
                            // full-resolution CPU pixel buffer costs ~8 MiB at 1080p.
                            match TextureBuffer::from_memory(
                                renderer,
                                &img,
                                Fourcc::Abgr8888,
                                image_size,
                                false,
                                1,
                                Transform::Normal,
                                None,
                            ) {
                                Ok(texture) => Some(texture),
                                Err(error) => {
                                    tracing::warn!(%error,"wallpaper upload failed");
                                    None
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "wallpaper could not be decoded");
                            None
                        }
                    });
            }
        }
        let mut elements = Vec::new();
        if cursor
            && state.backend_name == "native"
            && state.clipboard_capture.is_none()
            && !state.pointer_locked()
        {
            use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
            use smithay::reexports::wayland_server::Resource;
            let pos = state.seat.get_pointer().unwrap().current_location();
            if state.draw.is_some() {
                for (id, (x, y, w, h)) in self.crosshair_ids.iter().zip([
                    (pos.x as i32 - 8, pos.y as i32, 17, 1),
                    (pos.x as i32, pos.y as i32 - 8, 1, 17),
                ]) {
                    elements.push(
                        SolidColorRenderElement::new(
                            id.clone(),
                            Rectangle::new((x, y).into(), (w, h).into()),
                            self.commit,
                            color(&state.config.theme.selection).unwrap(),
                            Kind::Unspecified,
                        )
                        .into(),
                    );
                }
            } else if state.panels.contains(pos)
                || state.ui.contains(pos)
                || state.wifi.open
                || state.tray.open
                || state.launcher.open
                || state.screencast.prompt_open()
            {
                if let Ok(cursor) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    (pos.x - 1.0, pos.y - 1.0),
                    &self.cursor,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    elements.push(cursor.into());
                }
            } else if let CursorImageStatus::Surface(surface) = &state.cursor_status {
                if surface.is_alive() {
                    let hotspot = smithay::wayland::compositor::with_states(surface, |s| {
                        s.data_map
                            .get::<CursorImageSurfaceData>()
                            .map(|d| d.lock().unwrap().hotspot)
                            .unwrap_or_default()
                    });
                    elements.extend(render_elements_from_surface_tree::<_, SceneElement>(
                        renderer,
                        surface,
                        (pos - hotspot.to_f64()).to_physical_precise_round(1.0),
                        1.0,
                        1.0,
                        Kind::Cursor,
                    ));
                }
            } else if !matches!(state.cursor_status, CursorImageStatus::Hidden) {
                if let Ok(cursor) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    (pos.x - 1.0, pos.y - 1.0),
                    &self.cursor,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    elements.push(cursor.into());
                }
            }
        }
        if let Some(draw) = &state.draw {
            if let Some(start) = draw.start {
                let rect = layout::selection(start, draw.current, size);
                self.borders(
                    &mut elements,
                    rect,
                    2,
                    color(&state.config.theme.selection).unwrap(),
                    &self.selection_ids,
                );
                let alpha = state.config.theme.selection_opacity;
                if alpha > 0.0 {
                    let mut c = color(&state.config.theme.selection).unwrap();
                    for v in &mut c[..3] {
                        *v *= alpha;
                    }
                    c[3] = alpha;
                    elements.push(
                        SolidColorRenderElement::new(
                            self.selection_fill.clone(),
                            rect.to_physical_precise_round(1.0),
                            self.commit,
                            c,
                            Kind::Unspecified,
                        )
                        .into(),
                    );
                }
            }
        }
        for view in state
            .screencast
            .view
            .iter()
            .chain(state.launcher.view.iter())
            .chain(state.wifi.view.iter())
            .chain(state.tray.view.iter())
            .chain(state.volume.view.iter())
            .chain(state.ui.help.iter())
            .chain(state.ui.cards.iter())
        {
            if let Ok(element) = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (f64::from(view.rect.loc.x), f64::from(view.rect.loc.y)),
                &view.buffer,
                None,
                None,
                None,
                Kind::Unspecified,
            ) {
                elements.push(element.into());
            }
        }
        for panel in state.panels.views.iter().rev() {
            if let Ok(element) = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (f64::from(panel.rect.loc.x), f64::from(panel.rect.loc.y)),
                &panel.buffer,
                None,
                None,
                None,
                Kind::Unspecified,
            ) {
                elements.push(element.into());
            }
        }
        for window in state.space.elements().rev() {
            let location = state.space.element_location(window).unwrap() - window.geometry().loc;
            elements.extend(window.render_elements::<SceneElement>(
                renderer,
                (location.x, location.y).into(),
                1.0.into(),
                1.0,
            ));
            if let Some(m) = state.windows.iter().find(|m| &m.window == window) {
                if !m.fullscreen {
                    let b = state.config.theme.border_width;
                    // Use the client's committed geometry while it catches up to a configure.
                    let geo = window.geometry();
                    let rect = Rectangle::new(
                        (m.rect.loc.x - b, m.rect.loc.y - b).into(),
                        (geo.size.w + 2 * b, geo.size.h + 2 * b).into(),
                    );
                    let c = if state.focused.as_ref() == Some(window) {
                        &state.config.theme.active_border
                    } else {
                        &state.config.theme.inactive_border
                    };
                    self.borders(&mut elements, rect, b, color(c).unwrap(), &m.border_ids);
                }
            }
        }
        if let Some(buffer) = &self.wallpaper {
            elements.push(
                TextureRenderElement::from_texture_buffer(
                    self.wallpaper_location,
                    buffer,
                    None,
                    None,
                    None,
                    Kind::Unspecified,
                )
                .into(),
            );
        }
        elements
    }
    pub fn capture_cursor(
        &self,
        state: &Ember,
        renderer: &mut GlesRenderer,
        pos: Point<f64, Logical>,
        elements: &mut Vec<SceneElement>,
    ) {
        use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
        match &state.cursor_status {
            CursorImageStatus::Hidden => {}
            CursorImageStatus::Surface(surface) => {
                let hotspot = smithay::wayland::compositor::with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .hotspot
                });
                elements.extend(render_elements_from_surface_tree::<
                    GlesRenderer,
                    SceneElement,
                >(
                    renderer,
                    surface,
                    (pos - hotspot.to_f64()).to_physical_precise_round(1.0),
                    1.0,
                    1.0,
                    Kind::Cursor,
                ));
            }
            CursorImageStatus::Named(_) => {
                if let Ok(element) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    (pos.x - 1., pos.y - 1.),
                    &self.cursor,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    elements.push(element.into());
                }
            }
        }
    }
    fn borders(
        &self,
        elements: &mut Vec<SceneElement>,
        rect: Rect,
        width: i32,
        color: [f32; 4],
        ids: &[Id; 4],
    ) {
        if width <= 0 {
            return;
        }
        let (x, y, w, h) = (rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
        for (id, (x, y, w, h)) in ids.iter().zip([
            (x, y, w, width),
            (x, y + h - width, w, width),
            (x, y + width, width, (h - 2 * width).max(0)),
            (x + w - width, y + width, width, (h - 2 * width).max(0)),
        ]) {
            if w > 0 && h > 0 {
                elements.push(
                    SolidColorRenderElement::new(
                        id.clone(),
                        Rectangle::new((x, y).into(), (w, h).into()),
                        self.commit,
                        color,
                        Kind::Unspecified,
                    )
                    .into(),
                );
            }
        }
    }
}

fn default_cursor() -> MemoryRenderBuffer {
    let mut pixels = vec![0u8; 24 * 32 * 4];
    for y in 0..27usize {
        for x in 0..20usize {
            let arrow = y < 22 && x <= y * 2 / 3;
            let stem = (14..28).contains(&y) && x >= y / 3 - 1 && x <= y / 3 + 3;
            if arrow || stem {
                let i = (y * 24 + x) * 4;
                let edge = x == 0
                    || x == y * 2 / 3
                    || y == 21
                    || (stem && (x == y / 3 - 1 || x == y / 3 + 3));
                pixels[i..i + 4].copy_from_slice(if edge {
                    &[35, 15, 24, 255]
                } else {
                    &[240, 216, 223, 255]
                });
            }
        }
    }
    MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Abgr8888,
        (24, 32),
        1,
        Transform::Normal,
        None,
    )
}

// Runs only when a wallpaper is loaded, resized or its palette changes. The existing
// upload buffer is transformed in place and freed; idle holds only the GPU texture.
fn apply_wallpaper_palette(img: &mut image::RgbaImage, palette: &[String]) {
    let [shadow, highlight] = palette else { return };
    let shadow = color(shadow).expect("validated wallpaper shadow");
    let highlight = color(highlight).expect("validated wallpaper highlight");
    let shadow = shadow.map(|v| (v * 255.).round() as i32);
    let highlight = highlight.map(|v| (v * 255.).round() as i32);
    for pixel in img.pixels_mut() {
        let luma =
            (77 * i32::from(pixel[0]) + 150 * i32::from(pixel[1]) + 29 * i32::from(pixel[2]) + 128)
                >> 8;
        for channel in 0..3 {
            pixel[channel] =
                ((shadow[channel] * (255 - luma) + highlight[channel] * luma + 127) / 255) as u8;
        }
    }
}
#[cfg(test)]
mod wallpaper_palette_tests {
    use super::*;
    #[test]
    fn palette_preserves_alpha_and_original_pixels_when_disabled() {
        let mut img = image::RgbaImage::from_raw(
            3,
            1,
            vec![0, 0, 0, 80, 255, 255, 255, 190, 128, 128, 128, 255],
        )
        .unwrap();
        let original = img.clone();
        apply_wallpaper_palette(&mut img, &[]);
        assert_eq!(img, original);
        apply_wallpaper_palette(&mut img, &["#201018".into(), "#a06070".into()]);
        assert_eq!(img.get_pixel(0, 0).0, [32, 16, 24, 80]);
        assert_eq!(img.get_pixel(1, 0).0, [160, 96, 112, 190]);
        assert_eq!(img.get_pixel(2, 0).0, [96, 56, 68, 255]);
    }
}
