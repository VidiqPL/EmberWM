//! Short-lived audio jobs and an expiring native volume display.
use crate::{
    config::{color, Config},
    panels, ui,
};
use smithay::utils::{Logical, Rectangle, Size};
use std::{
    collections::VecDeque,
    path::Path,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct Runtime {
    pub view: Option<ui::View>,
    job: Option<Receiver<Result<(u32, bool), String>>>,
    queue: VecDeque<String>,
    until: Option<Instant>,
    value: Option<(u32, bool)>,
    generation: u64,
    size: (i32, i32),
    mic: bool,
    pub error: Option<String>,
}
impl Runtime {
    pub fn request(&mut self, action: &str) {
        if self.queue.len() < 32 {
            self.queue.push_back(action.into());
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some() || !self.queue.is_empty()
    }
    pub fn wake(&self) -> Option<Duration> {
        self.until
            .map(|t| t.saturating_duration_since(Instant::now()))
    }
    pub fn update(
        &mut self,
        c: &Config,
        path: &Path,
        generation: u64,
        size: Size<i32, Logical>,
    ) -> bool {
        let mut changed = false;
        if let Some(result) = self.job.as_ref().and_then(|r| match r.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Background task ended unexpectedly".into()))
            }
        }) {
            self.job = None;
            match result {
                Ok(value) => {
                    self.error = None;
                    self.value = Some(value);
                    self.until =
                        Some(Instant::now() + Duration::from_millis(c.audio.osd_duration_ms));
                    self.view = None;
                    changed = true;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.job.is_none() {
            if let Some(action) = self.queue.pop_front() {
                self.mic = action == "mic-mute";
                let (tx, rx) = mpsc::sync_channel(1);
                self.job = Some(rx);
                if let Err(e) = std::thread::Builder::new()
                    .name("ember-volume".into())
                    .stack_size(128 * 1024)
                    .spawn(move || {
                        let _ = tx.send(crate::audio::change_volume(&action));
                    })
                {
                    self.job = None;
                    self.error = Some(e.to_string());
                }
            }
        }
        if !c.audio.osd_enabled || self.until.is_some_and(|t| Instant::now() >= t) {
            changed |= self.view.take().is_some();
            self.until = None;
            self.value = None;
            crate::memory::request_reclaim();
            return changed;
        }
        if self.size != (size.w, size.h) {
            self.size = (size.w, size.h);
            changed |= self.view.take().is_some();
        }
        if self.generation != generation {
            self.generation = generation;
            changed |= self.view.take().is_some();
        }
        if self.view.is_none() {
            if let Some((value, muted)) = self.value {
                let width = 320.min(size.w.max(1));
                let height = 68.min(size.h.max(1));
                let rect = Rectangle::new(
                    ((size.w - width) / 2, (size.h - height - 64).max(0)).into(),
                    (width, height).into(),
                );
                if let Ok(font) = panels::load_font(&c.panels, path) {
                    let bg = color(&c.panels.background).unwrap();
                    let fg = color(&c.panels.foreground).unwrap();
                    let accent = color(&c.panels.accent).unwrap();
                    let label = if muted {
                        format!("Muted  {value}%")
                    } else {
                        format!(
                            "{}  {value}%",
                            if self.mic { "Microphone" } else { "Volume" }
                        )
                    };
                    let buffer = ui::surface(width, height, |p| {
                        panels::fill(p, (width, height), (0, 0, width, height), bg);
                        ui::text(p, (width, height), &font, &label, 14., 16, 10, fg);
                        panels::fill(p, (width, height), (16, 43, width - 32, 6), fg);
                        panels::fill(
                            p,
                            (width, height),
                            (16, 43, (width - 32) * value.min(100) as i32 / 100, 6),
                            accent,
                        );
                        let thumb = 16 + (width - 32) * value.min(100) as i32 / 100;
                        panels::fill(p, (width, height), (thumb - 3, 40, 6, 12), accent);
                    });
                    self.view = Some(ui::View::overlay(rect, buffer));
                    changed = true;
                }
            }
        }
        changed
    }
}
