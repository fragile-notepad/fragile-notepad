//! Data-device and optional xdg-toplevel-drag on winit's existing connection.
use super::WindowId;
use super::state::WinitState;
use crate::event::WindowEvent;
use crate::tab_drag::Event;
use ahash::AHashMap;
use sctk::compositor::CompositorState;
use sctk::reexports::client::backend::ObjectId;
use sctk::reexports::client::globals::GlobalList;
use sctk::reexports::client::protocol::{
    wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_seat::WlSeat,
    wl_surface::WlSurface,
};
use sctk::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use sctk::shell::WaylandSurface;
use sctk::shell::xdg::window::Window;
use sctk::shm::slot::{Buffer, SlotPool};
use std::sync::{Arc, Mutex};
use wayland_protocols::xdg::toplevel_drag::v1::client::{
    xdg_toplevel_drag_manager_v1, xdg_toplevel_drag_v1,
};

const MIME: &str = "application/x-fragile-notepad-tab-session";

pub struct State {
    manager: Option<wl_data_device_manager::WlDataDeviceManager>,
    toplevel_manager: Option<xdg_toplevel_drag_manager_v1::XdgToplevelDragManagerV1>,
    devices: AHashMap<ObjectId, wl_data_device::WlDataDevice>,
    active: Option<Active>,
    offers: AHashMap<ObjectId, wl_data_offer::WlDataOffer>,
    target_offer: Option<wl_data_offer::WlDataOffer>,
    target: Option<WindowId>,
}

struct Active {
    session: u64,
    origin: WindowId,
    source: wl_data_source::WlDataSource,
    toplevel: Option<xdg_toplevel_drag_v1::XdgToplevelDragV1>,
    following: Option<WlSurface>,
    dropped: bool,
    aborted: bool,
    icon: Option<WlSurface>,
    _icon_buffer: Option<Buffer>,
}

impl State {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<WinitState>) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            manager: globals.bind(qh, 3..=3, ()).ok(),
            toplevel_manager: globals.bind(qh, 1..=1, ()).ok(),
            devices: AHashMap::default(),
            active: None,
            offers: AHashMap::default(),
            target_offer: None,
            target: None,
        }))
    }

    pub fn capability(&self) -> Option<bool> {
        self.manager.as_ref()?;
        Some(self.toplevel_manager.is_some())
    }

    pub fn seat(&mut self, seat: &WlSeat, qh: &QueueHandle<WinitState>) {
        if let Some(manager) = &self.manager {
            self.devices.entry(seat.id()).or_insert_with(|| manager.get_data_device(seat, qh, ()));
        }
    }

    pub fn begin(
        &mut self,
        session: u64,
        origin: WindowId,
        window: &Window,
        seat: &WlSeat,
        serial: u32,
        qh: &QueueHandle<WinitState>,
        compositor: &CompositorState,
        pool: &Arc<Mutex<SlotPool>>,
    ) -> Option<bool> {
        if self.active.is_some() {
            return None;
        }
        let manager = self.manager.as_ref()?;
        let device = self.devices.get(&seat.id())?;
        let source = manager.create_data_source(qh, ());
        source.offer(MIME.into());
        source.set_actions(wl_data_device_manager::DndAction::Move);
        let toplevel = self
            .toplevel_manager
            .as_ref()
            .map(|manager| manager.get_xdg_toplevel_drag(&source, qh, ()));
        let supported = toplevel.is_some();
        let preview = make_preview(compositor, pool, qh);
        device.start_drag(
            Some(&source),
            window.wl_surface(),
            preview.as_ref().map(|(surface, _)| surface),
            serial,
        );
        if let Some((surface, buffer)) = &preview {
            let _ = buffer.attach_to(surface);
            surface.damage_buffer(0, 0, 160, 36);
            surface.commit();
        }
        let (icon, icon_buffer) = match preview {
            Some((surface, buffer)) => (Some(surface), Some(buffer)),
            None => (None, None),
        };
        self.active = Some(Active {
            session,
            origin,
            source: source.clone(),
            toplevel,
            following: None,
            dropped: false,
            aborted: false,
            icon,
            _icon_buffer: icon_buffer,
        });
        Some(supported)
    }

    pub fn follow(&mut self, session: u64, window: &Window, x: i32, y: i32) -> bool {
        let Some(active) = self
            .active
            .as_mut()
            .filter(|active| active.session == session && !active.dropped && !active.aborted)
        else {
            return false;
        };
        let Some(drag) = &active.toplevel else {
            return false;
        };
        if active.following.as_ref().is_some_and(|old| old == window.wl_surface()) {
            return true;
        }
        // A previous toplevel is unmapped when docking before another one is
        // attached; the protocol allows at most one active attachment.
        if let Some(old) = active.following.take().filter(Proxy::is_alive) {
            old.attach(None, 0, 0);
            old.commit();
        }
        if let Some(icon) = &active.icon {
            icon.attach(None, 0, 0);
            icon.commit();
        }
        drag.attach(window.xdg_toplevel(), x, y);
        active.following = Some(window.wl_surface().clone());
        true
    }

    pub fn window_hidden(&mut self, surface: &WlSurface) {
        let Some(active) = self.active.as_mut().filter(|active| !active.dropped && !active.aborted)
        else {
            return;
        };
        if active.following.as_ref() != Some(surface) {
            return;
        }
        active.following = None;
        if let (Some(icon), Some(buffer)) = (&active.icon, &active._icon_buffer) {
            let _ = buffer.attach_to(icon);
            icon.damage_buffer(0, 0, 160, 36);
            icon.commit();
        }
    }

    pub fn cancel(&mut self, session: u64) -> Option<WlSurface> {
        let active = self.active.as_mut().filter(|active| active.session == session)?;
        active.aborted = true;
        if let Some(icon) = &active.icon {
            icon.attach(None, 0, 0);
            icon.commit();
        }
        let following = active.following.take().filter(Proxy::is_alive);
        if let Some(following) = &following {
            following.attach(None, 0, 0);
            following.commit();
        }
        // Keep source/extension alive until the compositor sends a terminal
        // event. Destroying an ongoing xdg_toplevel_drag is a protocol error.
        following
    }
}

impl Dispatch<wl_data_device_manager::WlDataDeviceManager, ()> for WinitState {
    fn event(
        _: &mut Self,
        _: &wl_data_device_manager::WlDataDeviceManager,
        _: wl_data_device_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<xdg_toplevel_drag_manager_v1::XdgToplevelDragManagerV1, ()> for WinitState {
    fn event(
        _: &mut Self,
        _: &xdg_toplevel_drag_manager_v1::XdgToplevelDragManagerV1,
        _: xdg_toplevel_drag_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<xdg_toplevel_drag_v1::XdgToplevelDragV1, ()> for WinitState {
    fn event(
        _: &mut Self,
        _: &xdg_toplevel_drag_v1::XdgToplevelDragV1,
        _: xdg_toplevel_drag_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for WinitState {
    fn event(
        state: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut drag = state.tab_drag.lock().unwrap();
        match event {
            wl_data_device::Event::DataOffer { id } => {
                drag.offers.insert(id.id(), id);
            },
            wl_data_device::Event::Enter { serial, surface, x, y, id } => {
                let target = super::make_wid(&surface);
                let Some(active) =
                    drag.active.as_ref().filter(|active| !active.dropped && !active.aborted)
                else {
                    return;
                };
                let session = active.session;
                if let Some(offer) = &id {
                    offer.accept(serial, Some(MIME.into()));
                    if offer.version() >= 3 {
                        offer.set_actions(
                            wl_data_device_manager::DndAction::Move,
                            wl_data_device_manager::DndAction::Move,
                        );
                    }
                }
                drag.target = Some(target);
                drag.target_offer = id;
                if let Some(window) = state.windows.borrow().get(&target) {
                    let scale = window.lock().unwrap().scale_factor();
                    state.events_sink.push_window_event(
                        WindowEvent::TabDrag(Event::Motion {
                            session,
                            position: crate::dpi::PhysicalPosition::new(x * scale, y * scale),
                        }),
                        target,
                    );
                }
            },
            wl_data_device::Event::Motion { x, y, .. } => {
                if let (Some(active), Some(target)) = (
                    drag.active.as_ref().filter(|active| !active.dropped && !active.aborted),
                    drag.target,
                ) {
                    if let Some(window) = state.windows.borrow().get(&target) {
                        let scale = window.lock().unwrap().scale_factor();
                        state.events_sink.push_window_event(
                            WindowEvent::TabDrag(Event::Motion {
                                session: active.session,
                                position: crate::dpi::PhysicalPosition::new(x * scale, y * scale),
                            }),
                            target,
                        );
                    }
                }
            },
            wl_data_device::Event::Leave => {
                if let Some(active) =
                    drag.active.as_ref().filter(|active| !active.dropped && !active.aborted)
                {
                    state.events_sink.push_window_event(
                        WindowEvent::TabDrag(Event::Left { session: active.session }),
                        active.origin,
                    );
                }
                drag.target = None;
                if let Some(offer) = drag.target_offer.take() {
                    drag.offers.remove(&offer.id());
                    offer.destroy();
                }
            },
            wl_data_device::Event::Drop => {
                if let Some(offer) = drag.target_offer.take() {
                    if offer.version() >= 3 {
                        offer.finish();
                    }
                    drag.offers.remove(&offer.id());
                    offer.destroy();
                }
            },
            _ => {},
        }
    }
    sctk::reexports::client::event_created_child!(WinitState, wl_data_device::WlDataDevice, [0 => (wl_data_offer::WlDataOffer, ())]);
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for WinitState {
    fn event(
        _: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        _: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for WinitState {
    fn event(
        state: &mut Self,
        source: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut drag = state.tab_drag.lock().unwrap();
        if !drag.active.as_ref().is_some_and(|active| active.source == *source) {
            return;
        }
        match event {
            wl_data_source::Event::DndDropPerformed => {
                let active = drag.active.as_mut().unwrap();
                active.dropped = true;
                if !active.aborted {
                    state.events_sink.push_window_event(
                        WindowEvent::TabDrag(Event::Released { session: active.session }),
                        active.origin,
                    );
                }
                if let Some(toplevel) = active.toplevel.take() {
                    toplevel.destroy();
                }
            },
            wl_data_source::Event::Cancelled | wl_data_source::Event::DndFinished => {
                if let Some(mut active) = drag.active.take() {
                    if !active.dropped && !active.aborted {
                        state.events_sink.push_window_event(
                            WindowEvent::TabDrag(Event::Cancelled { session: active.session }),
                            active.origin,
                        );
                    }
                    if let Some(toplevel) = active.toplevel.take() {
                        toplevel.destroy();
                    }
                    if source.is_alive() {
                        source.destroy();
                    }
                }
            },
            wl_data_source::Event::Send { fd, .. } => {
                drop(fd);
            },
            _ => {},
        }
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        if let Some(icon) = self.icon.take().filter(Proxy::is_alive) {
            icon.destroy();
        }
    }
}

/// A small neutral tab silhouette keeps native fallback dragging visible even
/// while the pointer is between app surfaces. No renderer or second connection.
fn make_preview(
    compositor: &CompositorState,
    pool: &Arc<Mutex<SlotPool>>,
    qh: &QueueHandle<WinitState>,
) -> Option<(WlSurface, Buffer)> {
    let mut pool = pool.lock().unwrap();
    let (buffer, pixels) = pool
        .create_buffer(
            160,
            36,
            160 * 4,
            sctk::reexports::client::protocol::wl_shm::Format::Argb8888,
        )
        .ok()?;
    for y in 0..36usize {
        for x in 0..160usize {
            let outside = y < 8
                && ((x < 8 && (8 - x).pow(2) + (8 - y).pow(2) > 64)
                    || (x >= 152 && (x - 151).pow(2) + (8 - y).pow(2) > 64));
            let color: u32 = if outside {
                0
            } else if y < 2 {
                0xff77aaff
            } else if (x >= 16 && x < 28 && y >= 12 && y < 25)
                || (x >= 40 && x < 132 && y >= 14 && y < 17)
                || (x >= 40 && x < 108 && y >= 22 && y < 24)
            {
                0xffaab4c2
            } else {
                0xff303640
            };
            let offset = (y * 160 + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(&color.to_ne_bytes());
        }
    }
    Some((compositor.create_surface(qh), buffer))
}
