//! Shared popup layout, navigation, and input handling for context menus.

mod animation;

use std::time::{Duration, Instant};

use iced::advanced::Renderer as _;
use iced::advanced::text::Renderer as _;
use iced::advanced::{Layout, Shell, layout, mouse, overlay, renderer, text};
use iced::keyboard::{self, Key, key::Named};
use iced::{Background, Color, Event, Point, Rectangle, Renderer, Size, Theme};

use crate::core::{EditorSettings, ShortcutCommand, ShortcutDisplayPart, ShortcutModifierIcon};
use crate::message::Message;
use crate::ui::icons::hero::{self, HeroIcon};
use crate::ui::menu::{self, MenuNode, MenuShortcutHint};
use crate::ui::{motion, styles};

use animation::Transition;

const ROW_HEIGHT: f32 = 28.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const PADDING: f32 = 5.0;
const EDGE: f32 = 8.0;
const ENTRANCE_OFFSET: f32 = -5.0;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const WHEEL_EASING_DURATION: Duration = Duration::from_millis(150);

#[derive(Default, Clone, PartialEq)]
pub(super) struct State {
    anchor: Option<Point>,
    path: Vec<usize>,
    highlighted: Vec<Option<usize>>,
    offsets: Vec<f32>,
    scroll_motion: Option<ScrollMotion>,
    wheel_input: iced::widget::scrollable::WheelScrollInput,
    transitions: Vec<Transition>,
}

#[derive(Clone, PartialEq)]
struct ScrollMotion {
    depth: usize,
    origin: f32,
    target: f32,
    started: Instant,
}

impl State {
    pub(super) fn anchor(&self) -> Option<Point> {
        self.anchor
    }

    pub(super) fn is_open(&self) -> bool {
        self.anchor.is_some()
    }

    pub(super) fn open(&mut self, anchor: Point, entries: &[MenuNode], keyboard: bool) {
        self.anchor = Some(anchor);
        self.path.clear();
        self.highlighted = vec![keyboard.then(|| first_enabled(entries)).flatten()];
        self.offsets = vec![0.0];
        self.scroll_motion = None;
        self.wheel_input = iced::widget::scrollable::WheelScrollInput::default();
        self.transitions = vec![Transition::entering()];
    }

    pub(super) fn close(&mut self) {
        self.anchor = None;
        self.path.clear();
        self.highlighted.clear();
        self.offsets.clear();
        self.scroll_motion = None;
        self.transitions.clear();
    }

    fn dismiss(&mut self) {
        for transition in &mut self.transitions {
            transition.dismiss();
        }
        if self
            .transitions
            .first()
            .is_some_and(|root| root.is_hidden())
        {
            self.close();
        }
        self.scroll_motion = None;
    }

    fn is_closing(&self) -> bool {
        self.transitions
            .first()
            .is_some_and(|root| root.is_closing())
    }

    fn panel_progress(&self, depth: usize) -> f32 {
        self.transitions
            .get(depth)
            .map_or(1.0, |transition| transition.progress())
    }

    fn advance(&mut self, now: Instant) {
        for transition in &mut self.transitions {
            transition.advance(now);
        }
        if self
            .transitions
            .first()
            .is_some_and(|root| root.is_hidden())
        {
            self.close();
        }
    }

    fn highlight(&mut self, depth: usize, index: usize) {
        self.path.truncate(depth);
        self.highlighted.truncate(depth + 1);
        self.highlighted.resize(depth + 1, None);
        self.highlighted[depth] = Some(index);
        self.offsets.truncate(depth + 1);
        self.offsets.resize(depth + 1, 0.0);
        self.transitions.truncate(depth + 1);
        if self
            .scroll_motion
            .as_ref()
            .is_some_and(|motion| motion.depth > depth)
        {
            self.scroll_motion = None;
        }
    }

    fn enter(&mut self, depth: usize, index: usize, children: &[MenuNode], keyboard: bool) {
        if self.path.get(depth) == Some(&index) {
            return;
        }
        self.highlight(depth, index);
        self.path.push(index);
        self.highlighted
            .push(keyboard.then(|| first_enabled(children)).flatten());
        self.offsets.push(0.0);
        self.transitions.push(Transition::entering());
    }
}

pub(super) fn is_context_menu_key(event: &Event) -> bool {
    matches!(
        event,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::ContextMenu),
            ..
        })
    ) || matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::F10), modifiers, .. })
            if modifiers.shift() && !modifiers.command() && !modifiers.alt())
}

pub(super) struct ContextMenu<'a> {
    state: &'a mut State,
    entries: Vec<MenuNode>,
    settings: &'a EditorSettings,
    anchor: Point,
    shortcut_override: Option<(ShortcutCommand, Message)>,
}

impl<'a> ContextMenu<'a> {
    pub(super) fn new(
        state: &'a mut State,
        mut entries: Vec<MenuNode>,
        settings: &'a EditorSettings,
        anchor: Point,
    ) -> Self {
        text_shortcuts(&mut entries);
        Self {
            state,
            entries,
            settings,
            anchor,
            shortcut_override: None,
        }
    }

    pub(super) fn with_shortcut_override(
        mut self,
        command: ShortcutCommand,
        message: Message,
    ) -> Self {
        self.shortcut_override = Some((command, message));
        self
    }

    fn at_depth(&self, depth: usize) -> &[MenuNode] {
        entries_at_depth(&self.entries, &self.state.path, depth)
    }

    fn panel_bounds(&self, depth: usize, panel: Layout<'_>) -> Rectangle {
        let mut bounds = panel.bounds();
        bounds.y += ENTRANCE_OFFSET * (1.0 - self.state.panel_progress(depth));
        bounds
    }

    fn hit(&self, layout: Layout<'_>, point: Point) -> Option<(usize, usize)> {
        self.state.anchor?;
        if self.state.is_closing() {
            return None;
        }
        for (depth, panel) in layout
            .children()
            .take(self.state.path.len() + 1)
            .enumerate()
            .rev()
        {
            let bounds = self.panel_bounds(depth, panel);
            if bounds.contains(point) {
                let y = point.y - bounds.y - PADDING + self.state.offsets[depth];
                let mut top = 0.0;
                for (index, entry) in self.at_depth(depth).iter().enumerate() {
                    let bottom = top + row_height(entry);
                    if y >= top
                        && y < bottom
                        && point.y >= bounds.y + PADDING
                        && point.y < bounds.y + bounds.height - PADDING
                    {
                        return Some((depth, index));
                    }
                    top = bottom;
                }
                return None;
            }
        }
        None
    }

    fn activate(
        &mut self,
        depth: usize,
        index: usize,
        shell: &mut Shell<'_, Message>,
        keyboard: bool,
    ) {
        match self.at_depth(depth).get(index).cloned() {
            Some(MenuNode::Item { message, .. }) => {
                shell.publish(message);
                self.state.dismiss();
            }
            Some(MenuNode::Submenu { children, .. }) if children.iter().any(enabled) => {
                self.state.enter(depth, index, &children, keyboard);
            }
            _ => {}
        }
    }

    fn activate_shortcut(&mut self, command: ShortcutCommand, shell: &mut Shell<'_, Message>) {
        let message = self
            .shortcut_override
            .as_ref()
            .filter(|(candidate, _)| *candidate == command)
            .map(|(_, message)| message.clone())
            .unwrap_or(Message::Shortcut(command));
        shell.publish(message);
        self.state.dismiss();
    }

    fn reveal_highlight(&mut self, layout: Layout<'_>) {
        let depth = self.state.path.len();
        if let Some(index) = self.state.highlighted.get(depth).copied().flatten()
            && let Some(panel) = layout.children().nth(depth)
        {
            let top = rows_height(&self.at_depth(depth)[..index]);
            let height = (panel.bounds().height - 2.0 * PADDING).max(1.0);
            let offset = &mut self.state.offsets[depth];
            if top < *offset {
                *offset = top;
            }
            if top + ROW_HEIGHT > *offset + height {
                *offset = top + ROW_HEIGHT - height;
            }
        }
    }
}

impl overlay::Overlay<Message, Theme, Renderer> for ContextMenu<'_> {
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        if self.state.anchor.is_none() {
            return layout::Node::new(bounds);
        }
        let mut panels = Vec::new();
        let mut anchor = self.anchor;
        let mut parent: Option<Rectangle> = None;
        for depth in 0..=self.state.path.len() {
            let entries = self.at_depth(depth);
            let size = Size::new(
                menu::panel_width(entries, 264.0).min((bounds.width - EDGE * 2.0).max(1.0)),
                (rows_height(entries) + PADDING * 2.0).min((bounds.height - EDGE * 2.0).max(1.0)),
            );
            let mut rectangle = place_panel(anchor, size, bounds);
            if let Some(parent) = parent {
                let right = parent.x + parent.width - 1.0;
                rectangle.x = if right + size.width <= bounds.width - EDGE {
                    right
                } else {
                    (parent.x - size.width + 1.0).max(EDGE.min(bounds.width / 2.0))
                };
            }
            let maximum_offset = (rows_height(entries) + PADDING * 2.0 - size.height).max(0.0);
            self.state.offsets[depth] = self.state.offsets[depth].clamp(0.0, maximum_offset);
            if let Some(index) = self.state.path.get(depth) {
                anchor = Point::new(
                    rectangle.x + rectangle.width,
                    rectangle.y + rows_height(&self.at_depth(depth)[..*index])
                        - self.state.offsets[depth],
                );
            }
            parent = Some(rectangle);
            panels.push(layout::Node::new(size).move_to(rectangle.position()));
        }
        layout::Node::with_children(bounds, panels)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        if self.state.anchor.is_none() {
            return;
        }
        for (depth, panel) in layout
            .children()
            .take(self.state.path.len() + 1)
            .enumerate()
        {
            let opacity = self.state.panel_progress(0)
                * if depth == 0 {
                    1.0
                } else {
                    self.state.panel_progress(depth)
                };
            if opacity == 0.0 {
                continue;
            }
            let surface = motion::fade_container(styles::menu_dropdown_band(theme), opacity);
            let normal = surface
                .text_color
                .unwrap_or(Color::BLACK.scale_alpha(opacity));
            let muted = styles::menu_shortcut_hint_color(theme).scale_alpha(opacity);
            let bounds = self.panel_bounds(depth, panel);
            renderer.with_layer(bounds.expand(24.0), |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: surface.border,
                        shadow: surface.shadow,
                        ..Default::default()
                    },
                    surface
                        .background
                        .unwrap_or(Background::Color(Color::WHITE.scale_alpha(opacity))),
                );
                let clip = Rectangle {
                    x: bounds.x + PADDING,
                    y: bounds.y + PADDING,
                    width: (bounds.width - PADDING * 2.0).max(0.0),
                    height: (bounds.height - PADDING * 2.0).max(0.0),
                };
                renderer.with_layer(clip, |renderer| {
                    let mut y = bounds.y + PADDING - self.state.offsets[depth];
                    for (index, entry) in self.at_depth(depth).iter().enumerate() {
                        let height = row_height(entry);
                        let row = Rectangle {
                            x: clip.x,
                            y,
                            width: clip.width,
                            height,
                        };
                        y += height;
                        if !row.intersects(&clip) {
                            continue;
                        }
                        if matches!(entry, MenuNode::Separator) {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle {
                                        x: row.x + 7.0,
                                        y: row.center_y(),
                                        width: (row.width - 14.0).max(0.0),
                                        height: 1.0,
                                    },
                                    ..Default::default()
                                },
                                styles::separator(theme)
                                    .background
                                    .map(|background| background.scale_alpha(opacity))
                                    .unwrap_or(Background::Color(muted)),
                            );
                            continue;
                        }
                        let available = enabled(entry);
                        if available && self.state.highlighted.get(depth) == Some(&Some(index)) {
                            let highlight = motion::fade_container(
                                styles::menu_submenu_item(true)(theme),
                                opacity,
                            );
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: row,
                                    border: highlight.border,
                                    ..Default::default()
                                },
                                highlight
                                    .background
                                    .unwrap_or(Background::Color(Color::TRANSPARENT)),
                            );
                        }
                        let (label, shortcut) = match entry {
                            MenuNode::Item {
                                label, shortcut, ..
                            } => (
                                label,
                                shortcut.as_ref().map(shortcut_label).unwrap_or_default(),
                            ),
                            MenuNode::Disabled {
                                label, shortcut, ..
                            } => (
                                label,
                                shortcut.as_ref().map(shortcut_label).unwrap_or_default(),
                            ),
                            MenuNode::Submenu { label, .. } => (label, String::new()),
                            MenuNode::Separator => unreachable!(),
                        };
                        draw_text(
                            renderer,
                            label,
                            Point::new(row.x + 10.0, row.center_y()),
                            if available { normal } else { muted },
                            13.0,
                            text::Alignment::Left,
                            clip,
                        );
                        if matches!(entry, MenuNode::Submenu { .. }) {
                            draw_chevron(
                                renderer,
                                Point::new(row.x + row.width - 14.0, row.center_y()),
                                muted,
                                14.0,
                                0.0,
                                clip,
                            );
                        } else {
                            draw_text(
                                renderer,
                                &shortcut,
                                Point::new(row.x + row.width - 10.0, row.center_y()),
                                muted,
                                12.0,
                                text::Alignment::Right,
                                clip,
                            );
                        }
                    }
                });
                let total = rows_height(self.at_depth(depth));
                if self.state.offsets[depth] > 0.0 {
                    draw_chevron(
                        renderer,
                        Point::new(bounds.center_x(), bounds.y + 3.0),
                        muted,
                        9.0,
                        -std::f32::consts::FRAC_PI_2,
                        bounds,
                    );
                }
                if self.state.offsets[depth] + clip.height + 0.5 < total {
                    draw_chevron(
                        renderer,
                        Point::new(bounds.center_x(), bounds.y + bounds.height - 3.0),
                        muted,
                        9.0,
                        std::f32::consts::FRAC_PI_2,
                        bounds,
                    );
                }
            });
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        if self.state.anchor.is_none() {
            return;
        }
        if self.state.is_closing()
            && matches!(
                event,
                Event::Mouse(_) | Event::Keyboard(_) | Event::InputMethod(_) | Event::Touch(_)
            )
            && !matches!(event, Event::Keyboard(keyboard::Event::ModifiersChanged(_)))
        {
            // Keep the fading surface inert and prevent a second command or a
            // click reaching the editor before dismissal has finished.
            shell.capture_event();
            return;
        }
        let before = self.state.clone();
        match event {
            Event::Window(iced::window::Event::RedrawRequested(now)) => {
                self.state.advance(*now);
                if self
                    .state
                    .transitions
                    .iter()
                    .any(|transition| transition.needs_frames())
                {
                    shell.request_redraw_at(*now + FRAME_INTERVAL);
                }
                if let Some(motion) = self.state.scroll_motion.clone() {
                    if motion.depth <= self.state.path.len() {
                        let progress =
                            (now.saturating_duration_since(motion.started).as_secs_f32()
                                / WHEEL_EASING_DURATION.as_secs_f32())
                            .min(1.0);
                        self.state.offsets[motion.depth] = motion.origin
                            + (motion.target - motion.origin) * (1.0 - (1.0 - progress).powi(3));
                        if progress < 1.0 {
                            shell.request_redraw_at(*now + FRAME_INTERVAL);
                        } else {
                            self.state.scroll_motion = None;
                        }
                    } else {
                        self.state.scroll_motion = None;
                    }
                }
            }
            Event::Window(iced::window::Event::Unfocused | iced::window::Event::Resized(_)) => {
                self.state.close()
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some((depth, index)) = self.hit(layout, *position) {
                    let entry = self.at_depth(depth)[index].clone();
                    match entry {
                        MenuNode::Submenu { children, .. } if children.iter().any(enabled) => {
                            self.state.enter(depth, index, &children, false)
                        }
                        _ => self.state.highlight(depth, index),
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                self.state.scroll_motion = None;
                if let Some((depth, index)) =
                    cursor.position().and_then(|point| self.hit(layout, point))
                {
                    if *button == mouse::Button::Left {
                        self.activate(depth, index, shell, false);
                    }
                } else if !layout
                    .children()
                    .take(self.state.path.len() + 1)
                    .enumerate()
                    .any(|(depth, panel)| cursor.is_over(self.panel_bounds(depth, panel)))
                {
                    self.state.dismiss();
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some((depth, panel)) = layout
                    .children()
                    .take(self.state.path.len() + 1)
                    .enumerate()
                    .rfind(|(depth, panel)| cursor.is_over(self.panel_bounds(*depth, *panel)))
                {
                    let pixels = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y * ROW_HEIGHT * 3.0,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let maximum = (rows_height(self.at_depth(depth)) + PADDING * 2.0
                        - panel.bounds().height)
                        .max(0.0);
                    let origin = self.state.offsets[depth];
                    let pending = self
                        .state
                        .scroll_motion
                        .as_ref()
                        .filter(|motion| motion.depth == depth)
                        .map_or(origin, |motion| motion.target);
                    self.state.path.truncate(depth);
                    self.state.offsets.truncate(depth + 1);
                    self.state.highlighted.truncate(depth + 1);
                    self.state.transitions.truncate(depth + 1);
                    self.state.highlighted[depth] = None;
                    if self
                        .state
                        .wheel_input
                        .should_animate(*delta, std::time::Instant::now())
                    {
                        let base = if pixels * (pending - origin) > 0.0 {
                            origin
                        } else {
                            pending
                        };
                        let target = (base - pixels).clamp(0.0, maximum);
                        self.state.scroll_motion = (target != origin).then(|| ScrollMotion {
                            depth,
                            origin,
                            target,
                            started: std::time::Instant::now(),
                        });
                        if self.state.scroll_motion.is_some() {
                            shell.request_redraw();
                        }
                    } else {
                        self.state.scroll_motion = None;
                        // Direct input interrupts at the displayed position,
                        // without jumping through the unfinished wheel target.
                        self.state.offsets[depth] = (origin - pixels).clamp(0.0, maximum);
                    }
                }
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                modifiers,
                ..
            }) => {
                self.state.scroll_motion = None;
                let depth = self.state.path.len();
                let current = self.state.highlighted.get(depth).copied().flatten();
                let modified_shortcut = (!(modifiers.is_empty()
                    || key == &Key::Named(Named::Tab) && *modifiers == keyboard::Modifiers::SHIFT))
                    .then(|| {
                        self.settings
                            .shortcuts
                            .resolve(key, modified_key, *modifiers)
                    })
                    .flatten();
                if let Some(command) = modified_shortcut {
                    self.activate_shortcut(command, shell);
                } else {
                    match key.as_ref() {
                        Key::Named(
                            Named::Escape | Named::Tab | Named::ContextMenu | Named::F10,
                        ) => self.state.dismiss(),
                        Key::Named(Named::ArrowLeft) if depth > 0 => {
                            self.state.path.pop();
                            self.state.highlighted.truncate(depth);
                            self.state.offsets.truncate(depth);
                            self.state.transitions.truncate(depth);
                        }
                        Key::Named(Named::ArrowRight) => {
                            if let Some(index) = current
                                && matches!(
                                    self.at_depth(depth).get(index),
                                    Some(MenuNode::Submenu { .. })
                                )
                            {
                                self.activate(depth, index, shell, true);
                            }
                        }
                        Key::Named(Named::Enter | Named::Space) => {
                            if let Some(index) = current {
                                self.activate(depth, index, shell, true);
                            }
                        }
                        Key::Named(
                            Named::ArrowDown | Named::ArrowUp | Named::Home | Named::End,
                        ) => {
                            let entries = self.at_depth(depth);
                            let next = match key.as_ref() {
                                Key::Named(Named::Home) => first_enabled(entries),
                                Key::Named(Named::End) => entries.iter().rposition(enabled),
                                Key::Named(Named::ArrowUp) => next_enabled(entries, current, false),
                                _ => next_enabled(entries, current, true),
                            };
                            if let Some(index) = next {
                                self.state.highlight(depth, index);
                            }
                        }
                        _ => {
                            if let Some(command) =
                                self.settings
                                    .shortcuts
                                    .resolve(key, modified_key, *modifiers)
                            {
                                self.activate_shortcut(command, shell);
                            } else if let Key::Character(character) = key.as_ref() {
                                let entries = self.at_depth(depth);
                                let prefix = character.to_lowercase();
                                let start = current.map_or(0, |index| index + 1);
                                let next = (0..entries.len())
                                    .map(|step| (start + step) % entries.len())
                                    .find(|index| {
                                        enabled(&entries[*index])
                                            && node_label(&entries[*index])
                                                .to_lowercase()
                                                .starts_with(&prefix)
                                    });
                                if let Some(index) = next {
                                    self.state.highlight(depth, index);
                                }
                            }
                        }
                    }
                }
                self.reveal_highlight(layout);
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(_)) => return,
            Event::Keyboard(_) | Event::InputMethod(_) => {
                shell.capture_event();
            }
            _ => return,
        }
        if *self.state == before {
            return;
        }
        if self.state.anchor.is_none() {
            shell.invalidate_widgets();
        } else if self.state.path != before.path || self.state.offsets != before.offsets {
            shell.invalidate_layout();
        }
        if !matches!(
            event,
            Event::Window(iced::window::Event::RedrawRequested(_))
        ) {
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor
            .position()
            .and_then(|point| self.hit(layout, point))
            .is_some_and(|(depth, index)| enabled(&self.at_depth(depth)[index]))
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Idle
        }
    }
}

fn entries_at_depth<'a>(entries: &'a [MenuNode], path: &[usize], depth: usize) -> &'a [MenuNode] {
    let mut entries = entries;
    for index in path.iter().take(depth) {
        match entries.get(*index) {
            Some(MenuNode::Submenu { children, .. }) => entries = children,
            _ => return &[],
        }
    }
    entries
}

fn row_height(entry: &MenuNode) -> f32 {
    if matches!(entry, MenuNode::Separator) {
        SEPARATOR_HEIGHT
    } else {
        ROW_HEIGHT
    }
}

fn rows_height(entries: &[MenuNode]) -> f32 {
    entries.iter().map(row_height).sum()
}

fn enabled(entry: &MenuNode) -> bool {
    match entry {
        MenuNode::Item { .. } => true,
        MenuNode::Submenu { children, .. } => children.iter().any(enabled),
        _ => false,
    }
}

fn first_enabled(entries: &[MenuNode]) -> Option<usize> {
    entries.iter().position(enabled)
}

fn next_enabled(entries: &[MenuNode], current: Option<usize>, forward: bool) -> Option<usize> {
    let count = entries.len();
    (1..=count)
        .map(|step| {
            if forward {
                (current.unwrap_or(count - 1) + step) % count
            } else {
                (current.unwrap_or(0) + count - step) % count
            }
        })
        .find(|index| enabled(&entries[*index]))
}

fn node_label(entry: &MenuNode) -> &str {
    match entry {
        MenuNode::Item { label, .. }
        | MenuNode::Disabled { label, .. }
        | MenuNode::Submenu { label, .. } => label,
        MenuNode::Separator => "",
    }
}

fn place_panel(anchor: Point, size: Size, viewport: Size) -> Rectangle {
    let inset_x = EDGE.min((viewport.width - size.width).max(0.0) / 2.0);
    let inset_y = EDGE.min((viewport.height - size.height).max(0.0) / 2.0);
    Rectangle {
        x: anchor.x.clamp(
            inset_x,
            (viewport.width - size.width - inset_x).max(inset_x),
        ),
        y: anchor.y.clamp(
            inset_y,
            (viewport.height - size.height - inset_y).max(inset_y),
        ),
        width: size.width,
        height: size.height,
    }
}

fn text_shortcuts(entries: &mut [MenuNode]) {
    for entry in entries {
        match entry {
            MenuNode::Item {
                shortcut: Some(shortcut),
                ..
            }
            | MenuNode::Disabled {
                shortcut: Some(shortcut),
                ..
            } => {
                *shortcut = MenuShortcutHint::Text(shortcut_label(shortcut));
            }
            MenuNode::Submenu { children, .. } => text_shortcuts(children),
            _ => {}
        }
    }
}

fn shortcut_label(shortcut: &MenuShortcutHint) -> String {
    match shortcut {
        MenuShortcutHint::Text(label) => label.clone(),
        MenuShortcutHint::Binding(display) => display
            .modifiers
            .iter()
            .map(|part| match part {
                ShortcutDisplayPart::Text(text) => *text,
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Shift) => "Shift",
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Command) => "⌘",
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Option) => "⌥",
                ShortcutDisplayPart::Icon(ShortcutModifierIcon::Windows) => "Win",
            })
            .chain(std::iter::once(display.key.as_str()))
            .collect::<Vec<_>>()
            .join("+"),
    }
}

fn draw_chevron(
    renderer: &mut Renderer,
    center: Point,
    color: Color,
    size: f32,
    rotation: f32,
    clip: Rectangle,
) {
    hero::draw(
        renderer,
        HeroIcon::ChevronRight,
        Rectangle::new(
            Point::new(center.x - size / 2.0, center.y - size / 2.0),
            Size::new(size, size),
        ),
        clip,
        color,
        iced::Radians(rotation),
    );
}

fn draw_text(
    renderer: &mut Renderer,
    content: &str,
    position: Point,
    color: Color,
    size: f32,
    align_x: text::Alignment,
    clip: Rectangle,
) {
    renderer.fill_text(
        text::Text {
            content: content.to_owned(),
            bounds: Size::new(clip.width, ROW_HEIGHT),
            size: iced::Pixels(size),
            line_height: text::LineHeight::Relative(1.0),
            font: iced::Font::DEFAULT,
            align_x,
            align_y: iced::alignment::Vertical::Center,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: renderer.scale_factor(),
        },
        position,
        color,
        clip,
    );
}

#[cfg(test)]
mod tests;
