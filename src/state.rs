use std::{ffi::OsString, sync::Arc};

use smithay::{
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState},
    reexports::{
        calloop::{generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
            Display, DisplayHandle,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::XdgShellState,
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::CalloopData;

pub struct Ember {
    pub xwayland: crate::xwayland::Runtime,
    pub xwayland_shell: smithay::wayland::xwayland_shell::XWaylandShellState,
    pub image_capture: crate::image_capture::Runtime,
    pub foreign_toplevel_list: smithay::wayland::foreign_toplevel_list::ForeignToplevelListState,
    pub mouse: crate::mouse::Runtime,
    pub scratchpad: crate::scratchpad::Runtime,
    pub screencast: crate::screencast::Runtime,
    pub launcher: crate::launcher::Runtime,
    pub menu: crate::menu::Runtime,
    pub power: crate::power::Runtime,
    pub tray: crate::tray::Runtime,
    pub volume: crate::volume::Runtime,
    pub wifi: crate::wifi::Runtime,
    pub panels: crate::panels::Runtime,
    pub notifications: crate::notifications::Runtime,
    pub ui: crate::ui::Runtime,
    pub backend_name: &'static str,
    pub native_events: Vec<crate::native::NativeEvent>,
    pub session_active: bool,
    pub vt_request: Option<i32>,
    pub cursor_status: smithay::input::pointer::CursorImageStatus,
    pub audio_status: crate::audio::AudioStatus,
    pub running: bool,
    pub config: crate::config::Config,
    pub config_path: std::path::PathBuf,
    pub windows: Vec<crate::wm::ManagedWindow>,
    pub focused: Option<Window>,
    pub workspace: usize,
    pub dirty: bool,
    pub frames_submitted: u64,
    pub theme_generation: u64,
    pub draw: Option<crate::wm::DrawState>,
    pub drag: Option<crate::wm::DragState>,
    pub pending_draws:
        std::collections::HashMap<String, (crate::layout::Rect, usize, std::time::Instant)>,
    pub children: Vec<std::process::Child>,
    pub suppressed_keys: std::collections::HashSet<u32>,
    pub suppressed_buttons: std::collections::HashSet<u32>,
    pub screenshot: Option<std::path::PathBuf>,
    pub clipboard_capture: Option<crate::layout::Rect>,
    pub clipboard: crate::clipboard::Clipboard,
    pub dmabuf_state: smithay::wayland::dmabuf::DmabufState,
    pub dmabuf_pending: Vec<(
        smithay::backend::allocator::dmabuf::Dmabuf,
        smithay::wayland::dmabuf::ImportNotifier,
    )>,
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub _decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Ember>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,
}

impl Ember {
    pub fn new(
        event_loop: &mut EventLoop<CalloopData>,
        display: Display<Self>,
        config: crate::config::Config,
        config_path: std::path::PathBuf,
    ) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();
        configure_client_buffers(&dh);
        let xwayland_shell = smithay::wayland::xwayland_shell::XWaylandShellState::new::<Self>(&dh);
        let screencast = crate::screencast::Runtime::new(&dh, &config.screencast);

        let _relative_pointer =
            smithay::wayland::relative_pointer::RelativePointerManagerState::new::<Self>(&dh);
        let _constraints =
            smithay::wayland::pointer_constraints::PointerConstraintsState::new::<Self>(&dh);
        let foreign_toplevel_list =
            crate::image_capture::init(&dh, screencast.direct_capture.clone());
        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let decoration_state =
            smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let popups = PopupManager::default();

        // A seat is a group of keyboards, pointer and touch devices.
        // A seat typically has a pointer and maintains a keyboard focus and a pointer focus.
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "Ember");

        // Notify clients that we have a keyboard, for the sake of the example we assume that keyboard is always present.
        // You may want to track keyboard hot-plug in real compositor.
        seat.add_keyboard(
            smithay::input::keyboard::XkbConfig {
                layout: &config.keyboard.xkb_layout,
                ..Default::default()
            },
            config.keyboard.repeat_delay,
            config.keyboard.repeat_rate,
        )
        .unwrap();

        // Notify clients that we have a pointer (mouse)
        // Here we assume that there is always pointer plugged in
        seat.add_pointer();

        // A space represents a two-dimensional plane. Windows and Outputs can be mapped onto it.
        //
        // Windows get a position and stacking order through mapping.
        // Outputs become views of a part of the Space and can be rendered via Space::render_output.
        let space = Space::default();

        let socket_name = Self::init_wayland_listener(display, event_loop);

        // Get the loop signal, used to stop the event loop
        let loop_signal = event_loop.get_signal();

        Self {
            xwayland: Default::default(),
            xwayland_shell,
            image_capture: Default::default(),
            foreign_toplevel_list,
            mouse: Default::default(),
            scratchpad: Default::default(),
            screencast,
            launcher: crate::launcher::Runtime::default(),
            menu: Default::default(),
            power: Default::default(),
            tray: Default::default(),
            volume: Default::default(),
            wifi: crate::wifi::Runtime::default(),
            panels: crate::panels::Runtime::default(),
            notifications: crate::notifications::Runtime::default(),
            ui: crate::ui::Runtime::default(),
            backend_name: "nested",
            native_events: Vec::new(),
            session_active: true,
            vt_request: None,
            cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
            audio_status: crate::audio::AudioStatus::default(),
            running: true,
            config,
            config_path,
            windows: Vec::new(),
            focused: None,
            workspace: 0,
            dirty: true,
            frames_submitted: 0,
            theme_generation: 0,
            draw: None,
            drag: None,
            pending_draws: Default::default(),
            children: Vec::new(),
            suppressed_keys: Default::default(),
            suppressed_buttons: Default::default(),
            screenshot: None,
            clipboard_capture: None,
            clipboard: Default::default(),
            dmabuf_state: smithay::wayland::dmabuf::DmabufState::new(),
            dmabuf_pending: Vec::new(),
            start_time,
            display_handle: dh,

            space,
            loop_signal,
            socket_name,

            compositor_state,
            xdg_shell_state,
            _decoration_state: decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            seat,
        }
    }

    fn init_wayland_listener(
        display: Display<Ember>,
        event_loop: &mut EventLoop<CalloopData>,
    ) -> OsString {
        // Creates a new listening socket, automatically choosing the next available `wayland` socket name.
        let listening_socket = ListeningSocketSource::new_auto().unwrap();

        // Get the name of the listening socket.
        // Clients will connect to this socket.
        let socket_name = listening_socket.socket_name().to_os_string();

        let loop_handle = event_loop.handle();

        loop_handle
            .insert_source(listening_socket, move |client_stream, _, state| {
                let portal_capture = crate::screencast::trusted_peer(&client_stream);
                // Inside the callback, you should insert the client into the display.
                //
                // You may also associate some data with the client when inserting the client.
                state
                    .display_handle
                    .insert_client(
                        client_stream,
                        Arc::new(ClientState {
                            portal_capture,
                            ..Default::default()
                        }),
                    )
                    .unwrap();
            })
            .expect("Failed to init the wayland event source.");

        // You also need to add the display itself to the event loop, so that client events will be processed by wayland-server.
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // Safety: we don't drop the display
                    unsafe {
                        display
                            .get_mut()
                            .dispatch_clients(&mut state.state)
                            .unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .unwrap();

        socket_name
    }

    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space
            .element_under(pos)
            .and_then(|(window, location)| {
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
    }
}

#[derive(Default)]
pub struct ClientState {
    pub portal_capture: bool,
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        if matches!(reason, DisconnectReason::ProtocolError(_)) {
            tracing::warn!(?client_id, ?reason, "Wayland client protocol error");
        }
    }
}

fn configure_client_buffers(display: &DisplayHandle) {
    // A client may briefly block its GUI thread while encoding a clipboard image.
    // The Rust backend's 4 KiB default can disconnect it on an event burst.
    // This is a growth ceiling: buffers still start small and grow only on demand.
    display.set_default_max_buffer_size(1024 * 1024);
}

#[cfg(test)]
mod client_buffer_tests {
    use super::configure_client_buffers;
    use smithay::reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        Display,
    };
    use std::{
        io::{Read, Write},
        os::{fd::AsRawFd, unix::net::UnixStream},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    #[derive(Default)]
    struct Tracker(AtomicBool);
    impl ClientData for Tracker {
        fn disconnected(&self, _: ClientId, _: DisconnectReason) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    fn stalled_client(configure: bool) -> (bool, usize) {
        let mut display = Display::<()>::new().unwrap();
        let mut handle = display.handle();
        if configure {
            configure_client_buffers(&handle);
        }
        let (server, mut client) = UnixStream::pair().unwrap();
        let limit: libc::c_int = 4096;
        // A small socket send buffer reliably reproduces an application that
        // stops reading, without touching a real desktop or timing sleeps.
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    server.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&limit as *const libc::c_int).cast(),
                    std::mem::size_of_val(&limit) as libc::socklen_t,
                )
            },
            0
        );
        let tracker = Arc::new(Tracker::default());
        handle.insert_client(server, tracker.clone()).unwrap();
        const REQUESTS: u32 = 2048;
        let mut requests = Vec::new();
        for id in 2..REQUESTS + 2 {
            for value in [1u32, 12 << 16, id] {
                requests.extend(value.to_ne_bytes());
            }
        }
        client.write_all(&requests).unwrap();
        let _ = display.dispatch_clients(&mut ());
        if tracker.0.load(Ordering::Relaxed) {
            return (true, 0);
        }
        client.set_nonblocking(true).unwrap();
        let mut received = 0;
        let mut bytes = [0u8; 8192];
        // Sync returns callback.done plus display.delete_id, 12 bytes each.
        for _ in 0..100 {
            let _ = display.flush_clients();
            loop {
                match client.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => received += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("client read failed: {e}"),
                }
            }
            if received == REQUESTS as usize * 24 {
                break;
            }
        }
        (tracker.0.load(Ordering::Relaxed), received)
    }

    #[test]
    fn temporarily_stalled_client_survives_event_backpressure() {
        assert!(
            stalled_client(false).0,
            "baseline should reproduce disconnection"
        );
        let (disconnected, bytes) = stalled_client(true);
        assert!(
            !disconnected,
            "a short client stall must not close its connection"
        );
        assert_eq!(
            bytes,
            2048 * 24,
            "queued events must arrive intact after resuming"
        );
    }
}
