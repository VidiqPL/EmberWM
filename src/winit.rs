use crate::{config::color, render::Scene, CalloopData, Ember};
use smithay::{
    backend::{
        allocator::Fourcc,
        egl::EGLDevice,
        renderer::{
            damage::OutputDamageTracker, gles::GlesRenderer, ExportMem, Frame, ImportDma, Renderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        winit::{dpi::LogicalSize, window::WindowAttributes},
    },
    utils::{Rectangle, Transform},
};
use std::{path::Path, time::Duration};

pub struct Nested {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage: OutputDamageTracker,
    scene: Scene,
    presented: bool,
}
impl Nested {
    pub fn init(
        event_loop: &mut EventLoop<CalloopData>,
        data: &mut CalloopData,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (mut backend, events) = winit::init_from_attributes::<GlesRenderer>(
            WindowAttributes::default()
                .with_title("EmberWM — nested Wayland session")
                .with_inner_size(LogicalSize::new(1280., 800.)),
        )?;
        let renderer = backend.renderer();
        let formats = renderer.dmabuf_formats();
        let node = EGLDevice::device_for_display(renderer.egl_context().display())
            .and_then(|device| device.try_get_render_node());
        if let Ok(Some(node)) = node {
            let feedback =
                smithay::wayland::dmabuf::DmabufFeedbackBuilder::new(node.dev_id(), formats)
                    .build()?;
            data.state
                .dmabuf_state
                .create_global_with_default_feedback::<Ember>(&data.display_handle, &feedback);
        } else {
            data.state
                .dmabuf_state
                .create_global::<Ember>(&data.display_handle, formats);
        }
        let output = Output::new(
            "Ember-1".into(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Ember".into(),
                model: "Nested".into(),
            },
        );
        output.create_global::<Ember>(&data.display_handle);
        let mode = Mode {
            size: backend.window_size(),
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            Some(Transform::Flipped180),
            None,
            Some((0, 0).into()),
        );
        output.set_preferred(mode);
        data.state.space.map_output(&output, (0, 0));
        let event_output = output.clone();
        event_loop
            .handle()
            .insert_source(events, move |event, _, data| match event {
                WinitEvent::Resized { size, .. } => {
                    if size.w > 0 && size.h > 0 {
                        event_output.change_current_state(
                            Some(Mode {
                                size,
                                refresh: 60_000,
                            }),
                            None,
                            None,
                            None,
                        );
                        data.state.relayout();
                    }
                }
                WinitEvent::Input(event) => data.state.process_input_event(event),
                WinitEvent::Redraw => data.state.dirty = true,
                WinitEvent::CloseRequested => {
                    data.state.running = false;
                    data.state.loop_signal.stop();
                }
                _ => (),
            })?;
        Ok(Self {
            damage: OutputDamageTracker::from_output(&output),
            backend,
            output,
            scene: Scene::new(),
            presented: false,
        })
    }
    pub fn import_buffers(&mut self, data: &mut CalloopData) {
        for (buffer, notifier) in data.state.dmabuf_pending.drain(..) {
            if self.backend.renderer().import_dmabuf(&buffer, None).is_ok() {
                let _ = notifier.successful::<Ember>();
            } else {
                notifier.failed();
            }
        }
    }
    pub fn render(&mut self, data: &mut CalloopData) -> Result<(), Box<dyn std::error::Error>> {
        let size = self.backend.window_size();
        if size.w <= 0 || size.h <= 0 {
            return Ok(());
        }
        crate::image_capture::render(&mut data.state, &mut self.scene, self.backend.renderer());
        let no_cursor = crate::screencast::without_cursor(
            &mut self.scene,
            self.backend.renderer(),
            &data.state,
        );
        let state = &mut data.state;
        let screenshot = state.screenshot.take();
        let age = if screenshot.is_some()
            || state.clipboard_capture.is_some()
            || state.screencast.pending()
            || !self.presented
        {
            0
        } else {
            // Winit's bind returns a target; the GLES context becomes current when a frame starts.
            // Buffer age must be queried with the window surface current, including after imports.
            let (renderer, mut framebuffer) = self.backend.bind()?;
            let _ = renderer
                .render(&mut framebuffer, size, Transform::Flipped180)?
                .finish()?;
            drop(framebuffer);
            self.backend.buffer_age().unwrap_or(0)
        };
        let (renderer, mut framebuffer) = self.backend.bind()?;
        let elements = self.scene.elements(state, renderer);
        let damage = self
            .damage
            .render_output(
                renderer,
                &mut framebuffer,
                age,
                &elements,
                color(&state.config.theme.background).unwrap(),
            )?
            .damage
            .cloned();
        let mut readback = screenshot.is_some();
        if let Some(path) = screenshot {
            if let Err(error) = capture(
                renderer,
                &framebuffer,
                size.w,
                size.h,
                RowOrder::BottomFirst,
                &path,
            ) {
                tracing::warn!(%error,"screenshot failed");
            }
        }
        let copied = capture_clipboard(
            renderer,
            &framebuffer,
            size.w,
            size.h,
            RowOrder::BottomFirst,
            state,
        );
        readback |= copied;
        if state.screencast.pending() {
            let mapping = if state.screencast.needs_cursor() {
                crate::screencast::Readback::capture(renderer, &framebuffer, size.w, size.h, true)
                    .ok()
            } else {
                None
            };
            let pixels = mapping.as_ref().and_then(|m| m.pixels(renderer).ok());
            let without_cursor = no_cursor.as_ref().and_then(|m| m.pixels(renderer).ok());
            state.screencast.complete(
                pixels,
                without_cursor,
                (size.w, size.h).into(),
                state.start_time.elapsed(),
            );
            readback = true;
        }
        drop(no_cursor);
        if readback {
            let _ = renderer.cleanup_texture_cache();
            // Readback makes the context surfaceless; restore the window surface.
            let _ = renderer
                .render(&mut framebuffer, size, Transform::Flipped180)?
                .finish()?;
        }
        drop(framebuffer);
        data.display_handle.flush_clients()?;
        if let Some(damage) = damage {
            self.backend.submit(Some(&damage))?;
            state.frames_submitted += 1;
            self.presented = true;
        }
        state.space.elements().for_each(|window| {
            window.send_frame(
                &self.output,
                state.start_time.elapsed(),
                Some(Duration::ZERO),
                |_, _| Some(self.output.clone()),
            );
        });
        state.dirty = copied;
        Ok(())
    }
}
/// Readback row order depends on the render target, not GlesMapping::flipped().
/// Native Normal scanout is top-first; the Flipped180 window surface is bottom-first.
#[derive(Clone, Copy)]
pub(crate) enum RowOrder {
    TopFirst,
    BottomFirst,
}
pub(crate) fn capture(
    renderer: &mut GlesRenderer,
    framebuffer: &<GlesRenderer as smithay::backend::renderer::RendererSuper>::Framebuffer<'_>,
    w: i32,
    h: i32,
    rows: RowOrder,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    read_frame(renderer, framebuffer, w, h, rows)?.save(path)?;
    Ok(())
}
pub(crate) fn read_frame(
    renderer: &mut GlesRenderer,
    framebuffer: &<GlesRenderer as smithay::backend::renderer::RendererSuper>::Framebuffer<'_>,
    w: i32,
    h: i32,
    rows: RowOrder,
) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    let mapping = renderer.copy_framebuffer(
        framebuffer,
        Rectangle::from_size((w, h).into()),
        Fourcc::Abgr8888,
    )?;
    let bytes = renderer.map_texture(&mapping)?;
    let mut image = image::RgbaImage::from_raw(w as u32, h as u32, bytes.to_vec())
        .ok_or("invalid screenshot size")?;
    if matches!(rows, RowOrder::BottomFirst) {
        image::imageops::flip_vertical_in_place(&mut image);
    }
    Ok(image)
}

pub(crate) fn capture_clipboard(
    renderer: &mut GlesRenderer,
    framebuffer: &<GlesRenderer as smithay::backend::renderer::RendererSuper>::Framebuffer<'_>,
    w: i32,
    h: i32,
    rows: RowOrder,
    state: &mut Ember,
) -> bool {
    let Some(rect) = state.clipboard_capture.take() else {
        return false;
    };
    let result = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        use image::ImageEncoder;
        if rect.loc.x < 0
            || rect.loc.y < 0
            || rect.size.w < 1
            || rect.size.h < 1
            || rect.loc.x + rect.size.w > w
            || rect.loc.y + rect.size.h > h
        {
            return Err("output changed during screenshot selection".into());
        }
        // Read only the selected rectangle. A native capture can encode directly
        // from the GPU mapping, avoiding both a whole-output image and a crop clone.
        let y = if matches!(rows, RowOrder::BottomFirst) {
            h - rect.loc.y - rect.size.h
        } else {
            rect.loc.y
        };
        let mapping = renderer.copy_framebuffer(
            framebuffer,
            Rectangle::new((rect.loc.x, y).into(), (rect.size.w, rect.size.h).into()),
            Fourcc::Abgr8888,
        )?;
        let bytes = renderer.map_texture(&mapping)?;
        let bytes = if matches!(rows, RowOrder::BottomFirst) {
            let stride = rect.size.w as usize * 4;
            std::borrow::Cow::Owned(
                bytes
                    .chunks_exact(stride)
                    .rev()
                    .flatten()
                    .copied()
                    .collect::<Vec<u8>>(),
            )
        } else {
            std::borrow::Cow::Borrowed(bytes)
        };
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(
            &bytes,
            rect.size.w as u32,
            rect.size.h as u32,
            image::ExtendedColorType::Rgba8,
        )?;
        if png.len() > 16 * 1024 * 1024 {
            return Err("clipboard screenshot exceeds 16 MiB".into());
        }
        Ok(png)
    })();
    match result {
        Ok(png) => state.copy_screenshot(png),
        Err(error) => {
            let error = error.to_string();
            state.clipboard.error = Some(error.clone());
            state.capture_notice("Screenshot failed", &error);
        }
    }
    crate::memory::request_reclaim();
    true
}
