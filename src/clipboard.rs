//! Native image clipboard. Transfer workers exist only while a client pastes.
use crate::{layout::Rect, Ember};
use std::{
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone)]
pub enum SelectionData {
    Image(Arc<Vec<u8>>),
    X11(smithay::xwayland::xwm::XwmId),
}
#[derive(Default)]
pub struct Clipboard {
    pub png: Option<Arc<Vec<u8>>>,
    active: Arc<AtomicUsize>,
    pub error: Option<String>,
}
struct Transfer(Arc<AtomicUsize>);
impl Drop for Transfer {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
        crate::memory::request_reclaim();
    }
}
impl Clipboard {
    pub fn clear(&mut self) {
        self.png = None;
        crate::memory::request_reclaim();
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"mime_type":self.png.as_ref().map(|_|"image/png"),"bytes":self.png.as_ref().map_or(0,|p|p.len()),"allocated_bytes":self.png.as_ref().map_or(0,|p|p.capacity()),"active_transfers":self.active.load(Ordering::Relaxed),"error":self.error})
    }
    /// XWM polls its destination with epoll, which rejects regular files/memfds.
    /// Relay those destinations through a bounded, temporary pipe worker.
    pub fn x11_destination(&self, fd: OwnedFd) -> Result<OwnedFd, String> {
        use std::io::{Read, Write};
        use std::os::fd::FromRawFd;
        // SAFETY: fstat initializes the storage and only borrows the live fd.
        let regular = unsafe {
            let mut stat: libc::stat = std::mem::zeroed();
            libc::fstat(fd.as_raw_fd(), &mut stat) == 0
                && stat.st_mode & libc::S_IFMT == libc::S_IFREG
        };
        if !regular {
            return Ok(fd);
        }
        self.active
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < 4).then_some(n + 1)
            })
            .map_err(|_| "Clipboard transfer limit reached".to_string())?;
        let ticket = Transfer(self.active.clone());
        let mut raw = [-1; 2];
        // SAFETY: raw has storage for both returned descriptors.
        if unsafe { libc::pipe2(raw.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: pipe2 returned two newly owned descriptors, each wrapped once.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(raw[0]), OwnedFd::from_raw_fd(raw[1])) };
        std::thread::Builder::new()
            .name("ember-x11-file".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                let _ticket = ticket;
                let mut input = std::fs::File::from(read);
                let mut output = std::fs::File::from(fd);
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut buffer = [0u8; 16384];
                while Instant::now() < deadline {
                    let mut poll = libc::pollfd {
                        fd: input.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let remaining = deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(5000) as i32;
                    // SAFETY: poll points at one live descriptor entry.
                    if unsafe { libc::poll(&mut poll, 1, remaining) } <= 0 {
                        break;
                    }
                    match input.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            if output.write_all(&buffer[..n]).is_err() {
                                break;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(write)
    }
    pub fn send(&self, fd: OwnedFd, png: Arc<Vec<u8>>) {
        if self
            .active
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < 4).then_some(n + 1)
            })
            .is_err()
        {
            return;
        }
        let ticket = Transfer(self.active.clone());
        let _ = std::thread::Builder::new()
            .name("ember-clipboard".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                let _ticket = ticket;
                let raw = fd.as_raw_fd();
                // SAFETY: fd is owned for the whole transfer, and writes borrow valid PNG bytes.
                unsafe {
                    let flags = libc::fcntl(raw, libc::F_GETFL);
                    if flags < 0 || libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                        return;
                    }
                    let deadline = Instant::now() + Duration::from_secs(5);
                    let mut offset = 0;
                    while offset < png.len() && Instant::now() < deadline {
                        let n = libc::write(
                            raw,
                            png[offset..].as_ptr().cast(),
                            (png.len() - offset).min(65536),
                        );
                        if n > 0 {
                            offset += n as usize;
                            continue;
                        }
                        if n == 0 {
                            return;
                        }
                        let e = std::io::Error::last_os_error();
                        if e.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            return;
                        }
                        let mut p = libc::pollfd {
                            fd: raw,
                            events: libc::POLLOUT,
                            revents: 0,
                        };
                        libc::poll(&mut p, 1, 100);
                    }
                }
            });
    }
}
impl Ember {
    pub fn copy_screenshot(&mut self, mut png: Vec<u8>) {
        // The encoder grows geometrically; do not retain unused capacity in the clipboard.
        png.shrink_to_fit();
        let png = Arc::new(png);
        smithay::wayland::selection::data_device::set_data_device_selection(
            &self.display_handle,
            &self.seat,
            vec!["image/png".into()],
            SelectionData::Image(png.clone()),
        );
        self.x11_offer(Some(vec!["image/png".into()]));
        self.clipboard.png = Some(png);
        self.clipboard.error = None;
        self.capture_notice("Screenshot copied", "Paste the selected image with Ctrl+V.");
        crate::memory::request_reclaim();
    }
    pub fn capture_notice(&mut self, title: &str, body: &str) {
        self.notifications.notify(
            &self.config.notifications,
            crate::notifications::Request {
                app: "EmberWM",
                replaces: 0,
                summary: title,
                body,
                actions: vec![],
                urgency: 1,
                resident: false,
                timeout: 2000,
                sender: None,
            },
        );
    }
    pub fn request_region(&mut self, rect: Rect) -> Result<(), String> {
        let size = self.output_size();
        if rect.loc.x < 0
            || rect.loc.y < 0
            || rect.size.w < 1
            || rect.size.h < 1
            || rect
                .loc
                .x
                .checked_add(rect.size.w)
                .is_none_or(|x| x > size.w)
            || rect
                .loc
                .y
                .checked_add(rect.size.h)
                .is_none_or(|y| y > size.h)
        {
            return Err("screenshot region must fit the output".into());
        }
        self.clipboard_capture = Some(rect);
        self.dirty = true;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    #[test]
    fn clipboard_transfer_serves_exact_bytes_and_releases_worker() {
        let clipboard = Clipboard::default();
        let (writer, mut reader) = UnixStream::pair().unwrap();
        let bytes = Arc::new(vec![123; 512 * 1024]);
        clipboard.send(writer.into(), bytes.clone());
        let mut received = Vec::new();
        reader.read_to_end(&mut received).unwrap();
        assert_eq!(&received, bytes.as_ref());
        for _ in 0..100 {
            if clipboard.active.load(Ordering::Relaxed) == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("clipboard worker did not finish");
    }
}
