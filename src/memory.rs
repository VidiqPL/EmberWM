//! Return freed large UI/image allocations once the renderer has settled.
use std::sync::atomic::{AtomicBool, Ordering};
static PENDING: AtomicBool = AtomicBool::new(false);
pub fn request_reclaim() {
    PENDING.store(true, Ordering::Relaxed);
}
pub fn reclaim_if_idle(dirty: bool) {
    if !dirty && PENDING.swap(false, Ordering::Relaxed) {
        // glibc can otherwise retain freed wallpaper/font/surface arenas indefinitely.
        // This is event-driven (image change / overlay release), never a frame timer.
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0);
        }
    }
}
