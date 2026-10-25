//! A tab gesture is a reversible layout transaction; Document objects never move.
use crate::app::App;
use crate::core::session::SessionWindow;
use crate::core::{DocumentId, EditorWindowId};
use crate::message::Message;
use iced::window::tab_drag::{self, Capability, Event, SessionId};
use iced::{Point, Task, Vector, keyboard, window};

#[derive(Debug)]
pub(in crate::app) struct TabDragSession {
    pub pressed: DocumentId,
    id: SessionId,
    source: EditorWindowId,
    attached: EditorWindowId,
    selected: Vec<DocumentId>,
    layout: Vec<(EditorWindowId, crate::core::workspace::EditorWindow)>,
    placements: Vec<(EditorWindowId, Option<[f32; 4]>, bool, bool)>,
    start: Point,
    latest: (window::Id, Point),
    offset: Vector,
    grab_offset: f32,
    active: bool,
    collapse: bool,
    capability: Capability,
    following: Option<EditorWindowId>,
    hovered: Option<(window::Id, Point)>,
}

impl App {
    pub(in crate::app) fn press_tab(
        &mut self,
        document: DocumentId,
        point: Point,
        modifiers: keyboard::Modifiers,
    ) -> Task<Message> {
        let cancel = self.cancel_tab_drag();
        let source = self.workspace.current_window_id();
        if self.close_operation_pending()
            || self.session.waiting_for_startup()
            || self.workspace.owner_of(document) != Some(source)
        {
            return cancel;
        }
        self.menu.close();
        let layout = self
            .workspace
            .window_ids()
            .into_iter()
            .filter_map(|id| self.workspace.window(id).cloned().map(|state| (id, state)))
            .collect();
        let was_selected = self
            .workspace
            .window(source)
            .is_some_and(|w| w.selected_tabs.contains(&document));
        let toggle = if cfg!(target_os = "macos") {
            modifiers.command()
        } else {
            modifiers.control()
        };
        let previous_document = self.workspace.active_document_id();
        if modifiers.shift() {
            self.workspace.select_tab_range(source, document);
        } else if toggle {
            self.workspace.toggle_tab_selection(source, document);
        } else if !was_selected {
            self.workspace.select_in_window(source, document);
        } else {
            let selected = self
                .workspace
                .window(source)
                .map(|w| w.selected_tabs.clone())
                .unwrap_or_default();
            self.workspace.select_tabs(source, &selected, document);
        }
        let active_document = self.workspace.active_document_id();
        let auto_save = if active_document != previous_document {
            self.queue_auto_save(previous_document)
        } else {
            Task::none()
        };
        let selected = self
            .workspace
            .window(source)
            .map(|w| w.selected_tabs.clone())
            .unwrap_or_default();
        if !selected.contains(&document) {
            return Task::batch([cancel, auto_save, self.activate_document(active_document)]);
        }
        let Some(native) = self.main_window_id else {
            return Task::batch([cancel, auto_save]);
        };
        let tab_x = self
            .tab_geometry
            .get(&native)
            .and_then(|g| g.tabs.iter().find(|(id, _)| *id == document))
            .map_or(0.0, |(_, rect)| rect.x);
        let preceding_width = self.tab_geometry.get(&native).map_or(0.0, |g| {
            g.tabs
                .iter()
                .take_while(|(id, _)| *id != document)
                .filter(|(id, _)| selected.contains(id))
                .map(|(_, rect)| rect.width)
                .sum::<f32>()
        });
        let placements = self
            .editor_windows
            .iter()
            .map(|(id, w)| {
                (
                    *id,
                    w.bounds,
                    self.maximized_windows
                        .get(&w.native)
                        .copied()
                        .unwrap_or(false),
                    w.fullscreen,
                )
            })
            .collect();
        let id = SessionId(self.next_drag_session);
        self.next_drag_session = self.next_drag_session.wrapping_add(1);
        let grab_offset = point.x - tab_x + preceding_width;
        let strip_x = self.tab_geometry.get(&native).map_or(0.0, |g| g.bounds.x);
        self.tab_drag = Some(TabDragSession {
            pressed: document,
            id,
            source,
            attached: source,
            selected,
            layout,
            placements,
            start: point,
            latest: (native, point),
            offset: Vector::new(grab_offset + strip_x, point.y),
            grab_offset,
            active: false,
            collapse: was_selected && !toggle && !modifiers.shift(),
            capability: Capability::SurfaceOnly,
            following: None,
            hovered: Some((native, point)),
        });
        Task::batch([
            cancel,
            auto_save,
            self.activate_document(document),
            tab_drag::begin(native, id),
        ])
    }

    pub(in crate::app) fn move_tab_pointer(&mut self, point: Point) -> Task<Message> {
        // The native tracker remains authoritative after the pointer leaves the
        // source surface. Widget motion is useful before its first callback.
        let Some(drag) = &self.tab_drag else {
            return Task::none();
        };
        if drag.capability != Capability::SurfaceOnly {
            return Task::none();
        }
        let Some(native) = self.main_window_id else {
            return Task::none();
        };
        self.drag_motion(Some((native, point)), native, point)
    }

    pub(in crate::app) fn tab_drag_position(
        &self,
        native: window::Id,
    ) -> Option<crate::ui::tabs::DragPosition> {
        let drag = self.tab_drag.as_ref()?;
        let (hovered, point) = drag.hovered?;
        if !drag.active || drag.following.is_some() || hovered != native {
            return None;
        }
        let geometry = self.tab_geometry.get(&native)?;
        Some(geometry.drag_position(point, &drag.selected, drag.grab_offset))
    }

    pub(in crate::app) fn native_tab_drag(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Started {
                session,
                capability,
                ..
            } => {
                if let Some(drag) = self.tab_drag.as_mut().filter(|d| d.id == session) {
                    drag.capability = capability;
                }
                Task::none()
            }
            Event::Motion {
                session,
                window,
                position,
                hovered,
                ..
            } => {
                if self.tab_drag.as_ref().is_none_or(|d| d.id != session) {
                    return Task::none();
                }
                self.drag_motion(hovered, window, position)
            }
            Event::Released { session } => {
                if self.tab_drag.as_ref().is_none_or(|d| d.id != session) {
                    return Task::none();
                }
                self.finish_tab_drag()
            }
            Event::Cancelled { session, .. } => {
                if self.tab_drag.as_ref().is_none_or(|d| d.id != session) {
                    return Task::none();
                }
                self.cancel_tab_drag()
            }
        }
    }

    fn drag_motion(
        &mut self,
        hovered: Option<(window::Id, Point)>,
        position_window: window::Id,
        point: Point,
    ) -> Task<Message> {
        let Some(mut drag) = self.tab_drag.take() else {
            return Task::none();
        };
        drag.latest = (position_window, point);
        drag.hovered = hovered;
        if !drag.active && point.distance(drag.start) < 10.0 {
            self.tab_drag = Some(drag);
            return Task::none();
        }
        drag.active = true;
        let attached_native = self.editor_windows[&drag.attached].native;
        // Capture-owner coordinates remain valid outside the client area.
        let retained = (drag.following.is_none() && drag.capability == Capability::Desktop)
            .then(|| {
                let position = hovered
                    .filter(|(native, _)| *native == attached_native)
                    .map(|(_, position)| position)
                    .or_else(|| (position_window == attached_native).then_some(point))?;
                let geometry = self.tab_geometry.get(&attached_native)?;
                geometry.retains_drag(position).then_some((
                    drag.attached,
                    attached_native,
                    position,
                ))
            })
            .flatten();
        let target = hovered
            .and_then(|(native, position)| {
                let id = self.editor_id(native)?;
                let ui = self.window_ui(id)?;
                if ui.close_prompt.document().is_some()
                    || ui.go_to_line_prompt.is_some()
                    || ui.chrome_animation.about.target_visible()
                    || *ui.is_window_list_visible
                    || *ui.is_tab_list_visible
                {
                    return None;
                }
                let geometry = self.tab_geometry.get(&native)?;
                geometry.contains(position).then(|| {
                    (
                        id,
                        native,
                        geometry.drag_insertion(position, &drag.selected, drag.grab_offset),
                    )
                })
            })
            .or_else(|| {
                retained.map(|(id, native, position)| {
                    drag.hovered = Some((native, position));
                    let geometry = &self.tab_geometry[&native];
                    (
                        id,
                        native,
                        geometry.drag_insertion(position, &drag.selected, drag.grab_offset),
                    )
                })
            });
        if let Some((target, native, slot)) = target {
            if drag.following != Some(target) {
                let attaching = drag.attached != target || drag.following.is_some();
                self.workspace
                    .transfer_tabs(drag.attached, &drag.selected, target, slot);
                drag.attached = target;
                if !attaching {
                    self.tab_drag = Some(drag);
                    return Task::none();
                }
                let hidden = drag
                    .following
                    .take()
                    .and_then(|id| self.editor_windows.get(&id))
                    .map(|w| w.native);
                self.tab_drag = Some(drag);
                let hide = hidden
                    .map(|id| window::set_mode(id, window::Mode::Hidden))
                    .unwrap_or_else(Task::none);
                return tab_drag::attach(self.tab_drag.as_ref().unwrap().id, native)
                    .chain(hide)
                    .chain(window::gain_focus(native));
            }
        } else if drag.following.is_none()
            && drag.capability != Capability::SurfaceOnly
            // Ignore queued samples until capture reaches the attached window.
            && (drag.capability != Capability::Desktop || position_window == attached_native)
        {
            let all_source = drag.attached == drag.source
                && self.workspace.window_tab_ids(drag.source).len() == drag.selected.len();
            let (destination, open) = if all_source {
                (drag.source, Task::none())
            } else {
                let bounds = self.editor_windows.get(&drag.source).and_then(|w| w.bounds);
                let mut settings = window::Settings {
                    min_size: Some(iced::Size::new(640.0, 400.0)),
                    exit_on_close_request: false,
                    ..window::Settings::default()
                };
                if let Some([_, _, width, height]) = bounds {
                    settings.size = iced::Size::new(width, height);
                }
                // The runtime attaches Wayland drag metadata before mapping.
                settings.tab_drag = Some((drag.id, drag.offset));
                self.create_editor(true, Some(settings))
            };
            if destination != drag.attached {
                self.workspace
                    .transfer_tabs(drag.attached, &drag.selected, destination, 0);
            }
            drag.attached = destination;
            drag.following = Some(destination);
            let native = self.editor_windows[&destination].native;
            let session = drag.id;
            self.tab_drag = Some(drag);
            return if all_source {
                open.chain(Task::done(Message::TabDetachedReady(session, native)))
            } else {
                // New windows follow before show; only a reused source needs restoring.
                open
            };
        }
        self.tab_drag = Some(drag);
        Task::none()
    }

    pub(in crate::app) fn detached_tab_ready(
        &mut self,
        id: SessionId,
        native: window::Id,
    ) -> Task<Message> {
        let Some(drag) = self.tab_drag.as_ref().filter(|d| {
            d.id == id
                && d.following == Some(d.source)
                && d.following
                    .and_then(|id| self.editor_windows.get(&id))
                    .is_some_and(|w| w.native == native)
        }) else {
            return Task::none();
        };
        window::set_mode(native, window::Mode::Windowed)
            .chain(window::maximize(native, false))
            .chain(tab_drag::follow(id, native, drag.offset))
    }

    pub(in crate::app) fn update_tab_geometry(
        &mut self,
        geometry: crate::ui::tabs::StripGeometry,
    ) -> Task<Message> {
        let Some(native) = self.main_window_id else {
            return Task::none();
        };
        self.tab_geometry.insert(native, geometry);
        let Some(drag) = self
            .tab_drag
            .as_ref()
            .filter(|d| d.active && d.hovered.is_some_and(|(id, _)| id == native))
        else {
            return Task::none();
        };
        let (hovered, (position_window, latest)) = (drag.hovered, drag.latest);
        self.drag_motion(hovered, position_window, latest)
    }

    pub(in crate::app) fn cancel_tab_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.tab_drag.take() else {
            return Task::none();
        };
        self.workspace.restore_window_layout(&drag.layout);
        let mut tasks = Vec::new();
        for (id, bounds, maximized, fullscreen) in &drag.placements {
            if let Some(window) = self.editor_windows.get(id) {
                let native = window.native;
                tasks.push(window::set_mode(native, window::Mode::Windowed));
                if let Some([x, y, w, h]) = bounds {
                    tasks.push(window::resize(native, iced::Size::new(*w, *h)));
                    tasks.push(window::move_to(native, Point::new(*x, *y)));
                }
                tasks.push(window::maximize(native, *maximized));
                if *fullscreen {
                    tasks.push(window::set_mode(native, window::Mode::Fullscreen));
                }
            }
        }
        let temporary = self
            .workspace
            .window_ids()
            .into_iter()
            .filter(|id| !drag.layout.iter().any(|(original, _)| original == id))
            .collect::<Vec<_>>();
        for id in temporary {
            tasks.push(self.close_empty_editor(id));
        }
        self.switch_editor(drag.source);
        if let Some(native) = self.main_window_id {
            tasks.push(window::gain_focus(native));
        }
        let restore = tab_drag::cancel(drag.id).chain(Task::batch(tasks));
        self.resume_rendering_after_drag(restore)
    }

    fn finish_tab_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.tab_drag.take() else {
            return Task::none();
        };
        if !drag.active && drag.collapse {
            self.workspace.select_in_window(drag.source, drag.pressed);
        }
        let mut tasks = Vec::new();
        let empty = self
            .workspace
            .window_ids()
            .into_iter()
            .filter(|id| self.workspace.window_tab_ids(*id).is_empty())
            .collect::<Vec<_>>();
        for id in empty {
            tasks.push(self.close_empty_editor(id));
        }
        self.switch_editor(drag.attached);
        self.last_active_editor = Some(drag.attached);
        if let Some(native) = self.main_window_id {
            tasks.push(window::gain_focus(native));
        }
        tasks.push(self.request_session_save());
        self.resume_rendering_after_drag(Task::batch(tasks))
    }

    fn resume_rendering_after_drag(&mut self, task: Task<Message>) -> Task<Message> {
        if std::mem::take(&mut self.pending_drag_gpu_boost) {
            task.chain(Task::done(Message::BackendBoostRequested))
        } else {
            task
        }
    }

    pub(in crate::app) fn move_tab_to_new_window(&mut self, document: DocumentId) -> Task<Message> {
        let Some(source) = self.workspace.owner_of(document) else {
            return Task::none();
        };
        // Keep a registered source editor available if native creation fails.
        if self.workspace.window_tab_ids(source).len() <= 1 {
            return Task::none();
        }
        let (target, open) = self.create_editor(true, None);
        let moved = self.move_tab_between_windows(document, source, target);
        open.chain(moved)
    }

    pub(in crate::app) fn move_tab_to_window(
        &mut self,
        document: DocumentId,
        native: window::Id,
    ) -> Task<Message> {
        let (Some(source), Some(target)) =
            (self.workspace.owner_of(document), self.editor_id(native))
        else {
            return Task::none();
        };
        self.move_tab_between_windows(document, source, target)
    }

    fn move_tab_between_windows(
        &mut self,
        document: DocumentId,
        source: EditorWindowId,
        target: EditorWindowId,
    ) -> Task<Message> {
        if source == target {
            return Task::none();
        }
        let slot = self.workspace.window_tab_ids(target).len();
        if !self
            .workspace
            .transfer_tabs(source, &[document], target, slot)
        {
            return Task::none();
        }
        // A restored tab may become visible for the first time when its neighbor moves.
        let source_load = self
            .workspace
            .active_document_in(source)
            .map(|document| document.id)
            .map(|id| self.activate_document(id))
            .unwrap_or_else(Task::none);
        let close = if self.workspace.window_tab_ids(source).is_empty() {
            self.close_empty_editor(source)
        } else {
            Task::none()
        };
        self.switch_editor(target);
        self.last_active_editor = Some(target);
        let activate = self.activate_document(document);
        let focus = self
            .main_window_id
            .map(window::gain_focus)
            .unwrap_or_else(Task::none);
        Task::batch([source_load, close, activate, focus])
    }

    pub(in crate::app) fn move_selected_to_new_window(&mut self) -> Task<Message> {
        let cancel = self.cancel_tab_drag();
        let source = self.workspace.current_window_id();
        let ids = self
            .workspace
            .window(source)
            .map(|w| w.selected_tabs.clone())
            .unwrap_or_default();
        if ids.is_empty() {
            return cancel;
        }
        if ids.len() == self.workspace.window_tab_ids(source).len() {
            return cancel;
        }
        let (target, open) = self.create_editor(true, None);
        self.workspace.transfer_tabs(source, &ids, target, 0);
        self.switch_editor(target);
        self.last_active_editor = Some(target);
        Task::batch([cancel, open])
    }

    pub(in crate::app) fn move_selected_to_window(&mut self, native: window::Id) -> Task<Message> {
        let cancel = self.cancel_tab_drag();
        let Some(target) = self.editor_id(native) else {
            return cancel;
        };
        let source = self.workspace.current_window_id();
        let ids = self
            .workspace
            .window(source)
            .map(|w| w.selected_tabs.clone())
            .unwrap_or_default();
        let slot = self.workspace.window_tab_ids(target).len();
        if !self.workspace.transfer_tabs(source, &ids, target, slot) {
            return cancel;
        }
        let close = if self.workspace.window_tab_ids(source).is_empty() {
            self.close_empty_editor(source)
        } else {
            Task::none()
        };
        self.switch_editor(target);
        self.last_active_editor = Some(target);
        Task::batch([cancel, close, window::gain_focus(native)])
    }

    pub(in crate::app) fn drag_snapshot_windows(&self) -> Option<Vec<SessionWindow>> {
        let drag = self.tab_drag.as_ref()?;
        Some(
            drag.layout
                .iter()
                .filter(|(_, w)| !w.tabs.is_empty())
                .map(|(id, state)| {
                    let placement = drag
                        .placements
                        .iter()
                        .find(|(window, _, _, _)| window == id);
                    SessionWindow {
                        document_indices: state
                            .tabs
                            .iter()
                            .filter_map(|id| {
                                self.workspace.documents().iter().position(|d| d.id == *id)
                            })
                            .collect(),
                        active_index: state
                            .active_document_id
                            .and_then(|id| state.tabs.iter().position(|tab| *tab == id))
                            .unwrap_or(0),
                        bounds: placement.and_then(|(_, bounds, _, _)| *bounds),
                        maximized: placement.is_some_and(|(_, _, value, _)| *value),
                        fullscreen: placement.is_some_and(|(_, _, _, value)| *value),
                    }
                })
                .collect(),
        )
    }
}
