mod compositor;
mod xdg_shell;

use crate::Ember;

//
// Wl Seat
//

use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::{delegate_data_device, delegate_output, delegate_seat};

impl SeatHandler for Ember {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Ember> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        image: smithay::input::pointer::CursorImageStatus,
    ) {
        self.cursor_status = image;
        if self.backend_name == "native" {
            self.dirty = true;
        }
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(Ember);

//
// Wl Data Device
//

impl SelectionHandler for Ember {
    type SelectionUserData = crate::clipboard::SelectionData;
    fn new_selection(
        &mut self,
        ty: smithay::wayland::selection::SelectionTarget,
        _source: Option<smithay::wayland::selection::SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if matches!(ty, smithay::wayland::selection::SelectionTarget::Clipboard) {
            self.clipboard.clear();
            self.x11_offer(_source.map(|source| source.mime_types()));
        }
    }
    fn send_selection(
        &mut self,
        ty: smithay::wayland::selection::SelectionTarget,
        mime_type: String,
        fd: std::os::fd::OwnedFd,
        _seat: Seat<Self>,
        user_data: &Self::SelectionUserData,
    ) {
        if matches!(ty, smithay::wayland::selection::SelectionTarget::Clipboard) {
            match user_data {
                crate::clipboard::SelectionData::Image(png) if mime_type == "image/png" => {
                    self.clipboard.send(fd, png.clone())
                }
                crate::clipboard::SelectionData::X11(_) => self.x11_send(ty, mime_type, fd),
                _ => {}
            }
        }
    }
}

impl DataDeviceHandler for Ember {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Ember {}
impl ServerDndGrabHandler for Ember {}

delegate_data_device!(Ember);

//
// Wl Output & Xdg Output
//

impl OutputHandler for Ember {}
delegate_output!(Ember);

// Import asynchronously after dispatch, when the backend renderer is available.
impl smithay::wayland::dmabuf::DmabufHandler for Ember {
    fn dmabuf_state(&mut self) -> &mut smithay::wayland::dmabuf::DmabufState {
        &mut self.dmabuf_state
    }
    fn dmabuf_imported(
        &mut self,
        _: &smithay::wayland::dmabuf::DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: smithay::wayland::dmabuf::ImportNotifier,
    ) {
        self.dmabuf_pending.push((dmabuf, notifier));
    }
}
smithay::delegate_dmabuf!(Ember);

// Presentation feedback is advertised by the native backend only.
smithay::delegate_presentation!(Ember);
