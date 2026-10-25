//! Stable layout slots for hit testing, with independently animated tab paint.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Alignment, Element, Event, Length, Padding, Rectangle, Size, Theme, Vector, window};

use crate::core::DocumentId;
use crate::message::Message;

use super::DragPosition;

#[cfg(test)]
mod tests;

const DURATION: Duration = Duration::from_millis(160);

pub(super) fn view<'a>(
    tabs: Vec<(DocumentId, Element<'a, Message>)>,
    new_file: Element<'a, Message>,
    selected: Vec<DocumentId>,
    dragging: bool,
    drag: Option<DragPosition>,
) -> Element<'a, Message> {
    let (mut keys, mut children): (Vec<_>, Vec<_>) =
        tabs.into_iter().map(|(id, tab)| (Some(id), tab)).unzip();
    keys.push(None);
    children.push(new_file);
    Element::new(TabRow {
        keys,
        children,
        selected,
        dragging,
        drag,
    })
}

struct TabRow<'a> {
    keys: Vec<Option<DocumentId>>,
    children: Vec<Element<'a, Message>>,
    selected: Vec<DocumentId>,
    dragging: bool,
    drag: Option<DragPosition>,
}

struct State {
    keys: Vec<Option<DocumentId>>,
    positions: HashMap<Option<DocumentId>, Motion>,
    now: Instant,
}

#[derive(Debug)]
struct Motion {
    from: f32,
    target: f32,
    started: Instant,
}

impl Motion {
    fn new(x: f32, now: Instant) -> Self {
        Self {
            from: x,
            target: x,
            started: now,
        }
    }

    fn value(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.started).as_secs_f32()
            / DURATION.as_secs_f32())
        .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.target - self.from) * eased
    }

    fn retarget(&mut self, x: f32, now: Instant) {
        if self.target != x {
            self.from = self.value(now);
            self.target = x;
            self.started = now;
        }
    }

    fn active(&self, now: Instant) -> bool {
        self.from != self.target && now.saturating_duration_since(self.started) < DURATION
    }
}

impl TabRow<'_> {
    fn follows(&self, index: usize) -> bool {
        self.drag.is_some() && self.keys[index].is_some_and(|id| self.selected.contains(&id))
    }

    fn translation(
        &self,
        state: &State,
        layout: Layout<'_>,
        viewport: &Rectangle,
        index: usize,
    ) -> Vector {
        let bounds = layout.child(index).bounds();
        let x = if self.follows(index) {
            let drag = self.drag.unwrap();
            let preceding: f32 = self
                .keys
                .iter()
                .enumerate()
                .take(index)
                .filter(|(_, id)| id.is_some_and(|id| self.selected.contains(&id)))
                .map(|(i, _)| layout.child(i).bounds().width)
                .sum();
            let group_width = self
                .keys
                .iter()
                .enumerate()
                .filter(|(_, id)| id.is_some_and(|id| self.selected.contains(&id)))
                .map(|(i, _)| layout.child(i).bounds().width)
                .sum();
            // Scrollable passes an unscrolled layout and a translated viewport.
            drag.leading_x(group_width, viewport.width) + preceding + viewport.x - layout.bounds().x
        } else {
            state
                .positions
                .get(&self.keys[index])
                .map_or(bounds.x - layout.bounds().x, |motion| {
                    motion.value(state.now)
                })
        };
        Vector::new(layout.bounds().x + x - bounds.x, 0.0)
    }
}

impl Widget<Message, Theme, iced::Renderer> for TabRow<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            keys: Vec::new(),
            positions: HashMap::new(),
            now: Instant::now(),
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        let mut old: HashMap<_, _> = std::mem::take(&mut state.keys)
            .into_iter()
            .zip(std::mem::take(&mut tree.children))
            .collect();
        tree.children = self
            .keys
            .iter()
            .zip(&mut self.children)
            .map(|(key, child)| {
                let mut child_tree = old.remove(key).unwrap_or_else(Tree::empty);
                child_tree.diff(child.as_widget_mut());
                child_tree
            })
            .collect();
        state.keys.clone_from(&self.keys);
        state.positions.retain(|key, _| self.keys.contains(key));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Fixed(super::TAB_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = layout::flex::resolve(
            layout::flex::Axis::Horizontal,
            renderer,
            limits,
            Length::Shrink,
            Length::Fixed(super::TAB_HEIGHT),
            Padding::ZERO,
            0.0,
            Alignment::Center,
            &mut self.children,
            &mut tree.children,
        );
        let state = tree.state.downcast_mut::<State>();
        for (index, child) in node.children().iter().enumerate() {
            let motion = state
                .positions
                .entry(self.keys[index])
                .or_insert_with(|| Motion::new(child.bounds().x, state.now));
            if !self.follows(index) {
                motion.retarget(child.bounds().x, state.now);
            }
        }
        node
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for (index, (child, tree)) in
                self.children.iter_mut().zip(&mut tree.children).enumerate()
            {
                child
                    .as_widget_mut()
                    .operate(tree, layout.child(index), renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let was_animating = state
            .positions
            .values()
            .any(|motion| motion.active(state.now));
        state.now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        let translations: Vec<_> = (0..self.children.len())
            .map(|i| self.translation(state, layout, viewport, i))
            .collect();
        for (i, translation) in translations.iter().enumerate() {
            if self.follows(i) {
                let x = layout.child(i).bounds().x - layout.bounds().x + translation.x;
                state
                    .positions
                    .insert(self.keys[i], Motion::new(x, state.now));
            }
        }
        let animating = state
            .positions
            .values()
            .any(|motion| motion.active(state.now));
        if animating
            || was_animating && !matches!(event, Event::Window(window::Event::RedrawRequested(_)))
        {
            shell.request_redraw();
        }
        for (i, (child, child_tree)) in self.children.iter_mut().zip(&mut tree.children).enumerate()
        {
            let cursor = if self.dragging {
                cursor.levitate()
            } else {
                cursor - translations[i]
            };
            child.as_widget_mut().update(
                child_tree,
                event,
                layout.child(i),
                cursor,
                renderer,
                shell,
                &(*viewport - translations[i]),
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        self.children
            .iter()
            .zip(&tree.children)
            .enumerate()
            .map(|(i, (child, tree))| {
                let translation = self.translation(state, layout, viewport, i);
                child.as_widget().mouse_interaction(
                    tree,
                    layout.child(i),
                    cursor - translation,
                    &(*viewport - translation),
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        // Draw the held group last so neighboring tabs slide beneath it.
        for held in [false, true] {
            for (i, (child, tree)) in self.children.iter().zip(&tree.children).enumerate() {
                if self.follows(i) != held {
                    continue;
                }
                let translation = self.translation(state, layout, viewport, i);
                let Some(clip) = (layout.child(i).bounds() + translation).intersection(viewport)
                else {
                    continue;
                };
                let cursor = if self.dragging {
                    cursor.levitate()
                } else {
                    cursor - translation
                };
                // Isolate each tab so renderer batching cannot paint neighboring
                // text or icons above the held tab.
                renderer.with_layer(clip, |renderer| {
                    renderer.with_translation(translation, |renderer| {
                        child.as_widget().draw(
                            tree,
                            renderer,
                            theme,
                            style,
                            layout.child(i),
                            cursor,
                            &(*viewport - translation),
                        );
                    });
                });
            }
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let state = tree.state.downcast_ref::<State>();
        if self.dragging
            || state
                .positions
                .values()
                .any(|motion| motion.active(state.now))
        {
            return None;
        }
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
