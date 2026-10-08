//! Search controls exercised through widget events and the CPU renderer.
use std::time::{Duration, Instant};

use fragile_notepad::core::{Document, DocumentId, FindState};
use fragile_notepad::message::{AdvancedSearchTab, Message};
use fragile_notepad::search_dialog::SearchDialogState;
use fragile_notepad::ui::{advanced_search_panel, find_panel, motion};
use iced::advanced::{
    Layout, Shell,
    graphics::core::shell::Waker,
    layout, mouse,
    renderer::{self, Headless},
    widget::{self, Operation, Tree, operation},
};
use iced::{Color, Element, Event, Point, Rectangle, Size, Theme, keyboard};

#[derive(Default)]
struct Controls {
    labels: Vec<(String, Rectangle)>,
    inputs: Vec<(Option<widget::Id>, Rectangle)>,
}

impl Operation for Controls {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _id: Option<&widget::Id>, bounds: Rectangle, text: &str) {
        self.labels.push((text.to_owned(), bounds));
    }

    fn text_input(
        &mut self,
        id: Option<&widget::Id>,
        bounds: Rectangle,
        _state: &mut dyn operation::TextInput,
    ) {
        self.inputs.push((id.cloned(), bounds));
    }
}

impl Controls {
    fn label(&self, label: &str) -> Rectangle {
        self.labels
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("missing control label {label:?}"))
            .1
    }

    fn input(&self, id: &'static str) -> Rectangle {
        self.inputs
            .iter()
            .find(|(candidate, _)| candidate.as_ref() == Some(&widget::Id::new(id)))
            .unwrap_or_else(|| panic!("missing input {id:?}"))
            .1
    }
}

fn renderer() -> iced::Renderer {
    futures::executor::block_on(<iced::Renderer as Headless>::new(
        renderer::Settings::default(),
        Some("tiny-skia"),
    ))
    .expect("software renderer")
}

fn send(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &iced::Renderer,
    viewport: Rectangle,
    event: Event,
    cursor: mouse::Cursor,
) -> Vec<Message> {
    let mut messages = Vec::new();
    element.as_widget_mut().update(
        tree,
        &event,
        Layout::new(node),
        cursor,
        renderer,
        &mut Shell::new(&iced::window::Headless, Waker::noop(), &mut messages),
        &viewport,
    );
    messages
}

fn settle(
    element: &mut Element<'_, Message>,
    renderer: &iced::Renderer,
    size: Size,
) -> (Tree, layout::Node, Controls) {
    let viewport = Rectangle::with_size(size);
    let limits = layout::Limits::new(Size::ZERO, size);
    let mut tree = Tree::empty();
    tree.diff(element.as_widget_mut());
    let mut node = element.as_widget_mut().layout(&mut tree, renderer, &limits);
    let start = Instant::now();
    for at in [start, start + Duration::from_millis(200)] {
        send(
            element,
            &mut tree,
            &node,
            renderer,
            viewport,
            Event::Window(iced::window::Event::RedrawRequested(at)),
            mouse::Cursor::Unavailable,
        );
        node = element.as_widget_mut().layout(&mut tree, renderer, &limits);
    }
    let mut controls = Controls::default();
    element
        .as_widget_mut()
        .operate(&mut tree, Layout::new(&node), renderer, &mut controls);
    (tree, node, controls)
}

fn visible(bounds: Rectangle, viewport: Rectangle) {
    assert!(
        bounds.width > 0.0 && bounds.height > 0.0,
        "empty control: {bounds:?}"
    );
    for point in [
        Point::new(bounds.x + 0.01, bounds.y + 0.01),
        Point::new(
            bounds.x + bounds.width - 0.01,
            bounds.y + bounds.height - 0.01,
        ),
    ] {
        assert!(
            viewport.contains(point),
            "control outside viewport: {bounds:?}"
        );
    }
}

fn click(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &iced::Renderer,
    viewport: Rectangle,
    bounds: Rectangle,
) -> Vec<Message> {
    visible(bounds, viewport);
    let mut messages = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        messages.extend(send(
            element,
            tree,
            node,
            renderer,
            viewport,
            Event::Mouse(event),
            mouse::Cursor::Available(bounds.center()),
        ));
    }
    messages
}

fn key(named: keyboard::key::Named, modifiers: keyboard::Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(named),
        modified_key: keyboard::Key::Named(named),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

fn type_into(
    element: &mut Element<'_, Message>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &iced::Renderer,
    viewport: Rectangle,
    id: &'static str,
) -> Vec<Message> {
    element.as_widget_mut().operate(
        tree,
        Layout::new(node),
        renderer,
        &mut operation::focusable::focus::<()>(id.into()),
    );
    element.as_widget_mut().operate(
        tree,
        Layout::new(node),
        renderer,
        &mut operation::text_input::select_all::<()>(id.into()),
    );
    send(
        element,
        tree,
        node,
        renderer,
        viewport,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("x".into()),
            modified_key: keyboard::Key::Character("x".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyX),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: Some("x".into()),
            repeat: false,
        }),
        mouse::Cursor::Unavailable,
    )
}

fn snapshot(
    name: &str,
    element: &Element<'_, Message>,
    tree: &Tree,
    node: &layout::Node,
    renderer: &mut iced::Renderer,
    size: Size<u32>,
    theme: &Theme,
) {
    let viewport = Rectangle::with_size(Size::new(size.width as f32, size.height as f32));
    renderer::Renderer::reset(renderer, viewport);
    element.as_widget().draw(
        tree,
        renderer,
        theme,
        &renderer::Style::default(),
        Layout::new(node),
        mouse::Cursor::Unavailable,
        &viewport,
    );
    let pixels = renderer.screenshot(size, 1.0, Color::TRANSPARENT);
    assert_eq!(pixels.len(), size.width as usize * size.height as usize * 4);
    if std::env::var_os("FRAGILE_SEARCH_SNAPSHOTS").is_some() {
        std::fs::create_dir_all("target/search-review").expect("search review directory");
        tiny_skia::Pixmap::from_vec(
            pixels,
            tiny_skia::IntSize::from_wh(size.width, size.height).unwrap(),
        )
        .expect("RGBA search snapshot")
        .save_png(format!("target/search-review/{name}.png"))
        .expect("write search snapshot");
    }
}

#[test]
fn advanced_search_controls_fit_and_dispatch_in_every_scope_and_theme() {
    let documents = [
        Document::from_path(
            DocumentId::new(901),
            "search.rs",
            "fn main() {\n    let needle = \"a needle in a haystack\";\n    println!(\"{needle}\");\n}\n",
        ),
        Document::from_path(DocumentId::new(902), "notes.txt", "A second needle.\n"),
    ];
    let mut renderer = renderer();
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        for (tab_name, tab, replace, open) in [
            ("find", AdvancedSearchTab::Find, false, false),
            ("replace", AdvancedSearchTab::Replace, true, false),
            ("find-open", AdvancedSearchTab::FindInFiles, false, true),
            (
                "replace-open",
                AdvancedSearchTab::ReplaceInFiles,
                true,
                true,
            ),
        ] {
            for display in [false, true] {
                let mut dialog = SearchDialogState::new();
                dialog.active_tab = tab;
                dialog.query = "needle".into();
                dialog.replacement = "thread".into();
                dialog.include_pattern = "*.rs; *.txt".into();
                dialog.result_options_visible = display;
                dialog.refresh_from_documents(documents.iter().take(if open { 2 } else { 1 }));
                dialog.selected_result = Some(0);
                let size = Size::new(720.0, 560.0);
                let viewport = Rectangle::with_size(size);
                let mut element = advanced_search_panel::view(&dialog);
                let (mut tree, node, controls) = settle(&mut element, &renderer, size);
                assert_eq!(node.size(), size);
                for (_, bounds) in &controls.inputs {
                    visible(*bounds, viewport);
                }
                assert!(controls.input(advanced_search_panel::QUERY_INPUT_ID).width >= 180.0);
                assert_eq!(
                    controls.inputs.iter().any(|(id, _)| id.as_ref()
                        == Some(&widget::Id::new(
                            advanced_search_panel::REPLACEMENT_INPUT_ID
                        ))),
                    replace,
                    "collapsed replacement must stay out of focus traversal"
                );
                snapshot(
                    &format!("advanced-{tab_name}-{theme_name}-display-{display}"),
                    &element,
                    &tree,
                    &node,
                    &mut renderer,
                    Size::new(720, 560),
                    &theme,
                );
                if tab == AdvancedSearchTab::Find && !display {
                    for (name, status) in [("idle", "No query"), ("loading", "Searching…")] {
                        let mut status_dialog = SearchDialogState::new();
                        status_dialog.status = status.into();
                        if name == "loading" {
                            status_dialog.query = "needle".into();
                        }
                        let mut status_view = advanced_search_panel::view(&status_dialog);
                        let (status_tree, status_node, _) =
                            settle(&mut status_view, &renderer, size);
                        snapshot(
                            &format!("advanced-status-{name}-{theme_name}"),
                            &status_view,
                            &status_tree,
                            &status_node,
                            &mut renderer,
                            Size::new(720, 560),
                            &theme,
                        );
                    }
                }
                let labels = [
                    "Aa", "ab", "Find all", "Count", "Display", "Close", "Replace",
                ];
                for label in labels {
                    let messages = click(
                        &mut element,
                        &mut tree,
                        &node,
                        &renderer,
                        viewport,
                        controls.label(label),
                    );
                    assert!(
                        matches!(messages.as_slice(), [message] if match (label, message) {
                            ("Aa", Message::AdvancedSearchCaseSensitiveToggled(true)) => true,
                            ("ab", Message::AdvancedSearchWholeWordToggled(true)) => true,
                            ("Find all", Message::AdvancedFindAllOpenRun) => open,
                            ("Find all", Message::AdvancedFindAllCurrentRun) => !open,
                            ("Count", Message::AdvancedCountRun) => true,
                            ("Display", Message::AdvancedResultOptionsToggled) => true,
                            ("Close", Message::AdvancedSearchClosed) => true,
                            ("Replace", Message::AdvancedSearchTabSelected(selected)) => *selected == if open { AdvancedSearchTab::ReplaceInFiles } else { AdvancedSearchTab::Replace },
                            _ => false,
                        }),
                        "{tab_name}/{theme_name}/{label}: {messages:?}"
                    );
                }
                if replace {
                    let messages = click(
                        &mut element,
                        &mut tree,
                        &node,
                        &renderer,
                        viewport,
                        controls.label("Replace all"),
                    );
                    assert!(
                        matches!(messages.as_slice(), [Message::AdvancedReplaceAllOpenRun] if open)
                            || matches!(messages.as_slice(), [Message::AdvancedReplaceAllCurrentRun] if !open)
                    );
                }
                if display {
                    assert!(matches!(
                        click(
                            &mut element,
                            &mut tree,
                            &node,
                            &renderer,
                            viewport,
                            controls.label("Reset")
                        )
                        .as_slice(),
                        [Message::AdvancedResultOptionsReset]
                    ));
                }
                let messages = click(
                    &mut element,
                    &mut tree,
                    &node,
                    &renderer,
                    viewport,
                    controls.label(if open {
                        "Current document"
                    } else {
                        "Open documents"
                    }),
                );
                let other_scope = match (replace, open) {
                    (false, false) => AdvancedSearchTab::FindInFiles,
                    (true, false) => AdvancedSearchTab::ReplaceInFiles,
                    (false, true) => AdvancedSearchTab::Find,
                    (true, true) => AdvancedSearchTab::Replace,
                };
                assert!(
                    matches!(messages.as_slice(), [Message::AdvancedSearchTabSelected(tab)] if *tab == other_scope)
                );
                let first = &dialog.results[0];
                let start = first.selection.range().start;
                let location = format!("{}:{}", start.line + 1, start.column + 1);
                let messages = click(
                    &mut element,
                    &mut tree,
                    &node,
                    &renderer,
                    viewport,
                    controls.label(&location),
                );
                assert!(
                    matches!(messages.as_slice(), [Message::AdvancedSearchResultSelected(id, selection)] if *id == first.document_id && *selection == first.selection)
                );
                assert!(
                    matches!(type_into(&mut element, &mut tree, &node, &renderer, viewport, advanced_search_panel::QUERY_INPUT_ID).as_slice(), [Message::AdvancedSearchQueryChanged(query)] if query == "x")
                );
                if replace {
                    assert!(
                        matches!(type_into(&mut element, &mut tree, &node, &renderer, viewport, advanced_search_panel::REPLACEMENT_INPUT_ID).as_slice(), [Message::AdvancedSearchReplacementChanged(replacement)] if replacement == "x")
                    );
                }
                element.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut operation::focusable::focus::<()>(
                        advanced_search_panel::QUERY_INPUT_ID.into(),
                    ),
                );
                for modifiers in [
                    keyboard::Modifiers::empty(),
                    keyboard::Modifiers::SHIFT,
                    keyboard::Modifiers::CTRL,
                ] {
                    assert!(
                        send(
                            &mut element,
                            &mut tree,
                            &node,
                            &renderer,
                            viewport,
                            key(keyboard::key::Named::Enter, modifiers),
                            mouse::Cursor::Unavailable
                        )
                        .is_empty(),
                        "runtime search keys must not receive duplicate widget submissions"
                    );
                }
            }
        }
    }
}

#[test]
fn count_summary_shows_exact_totals_and_returns_to_matching_lines() {
    let documents = [
        Document::from_path(DocumentId::new(911), "many.txt", &"needle ".repeat(620)),
        Document::from_path(DocumentId::new(912), "none.txt", "No matching text."),
    ];
    let mut renderer = renderer();
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        for open in [false, true] {
            for display in [false, true] {
                for no_matches in [false, true] {
                    let mut dialog = SearchDialogState::new();
                    dialog.active_tab = if open {
                        AdvancedSearchTab::ReplaceInFiles
                    } else {
                        AdvancedSearchTab::Replace
                    };
                    dialog.query = if no_matches { "absent" } else { "needle" }.into();
                    dialog.result_options_visible = display;
                    if !no_matches && !display {
                        dialog.refresh_from_documents(documents.iter().take(if open {
                            2
                        } else {
                            1
                        }));
                        let size = Size::new(720.0, 560.0);
                        let mut limited = advanced_search_panel::view(&dialog);
                        let (tree, node, controls) = settle(&mut limited, &renderer, size);
                        visible(controls.label("500+"), Rectangle::with_size(size));
                        visible(controls.label("Limit reached"), Rectangle::with_size(size));
                        snapshot(
                            &format!("advanced-limited-{theme_name}-open-{open}"),
                            &limited,
                            &tree,
                            &node,
                            &mut renderer,
                            Size::new(720, 560),
                            &theme,
                        );
                    }
                    dialog.count_from_documents(documents.iter().take(if open { 2 } else { 1 }));
                    let size = Size::new(720.0, 560.0);
                    let viewport = Rectangle::with_size(size);
                    let mut element = advanced_search_panel::view(&dialog);
                    let (mut tree, node, controls) = settle(&mut element, &renderer, size);
                    assert_eq!(node.size(), size);
                    visible(controls.label("Full count"), viewport);
                    visible(
                        controls.label(if no_matches { "0" } else { "620" }),
                        viewport,
                    );
                    controls.label("many.txt");
                    if open {
                        controls.label("none.txt");
                    }
                    if no_matches {
                        assert!(
                            !controls
                                .labels
                                .iter()
                                .any(|(label, _)| label == "Show matching lines")
                        );
                    } else {
                        let messages = click(
                            &mut element,
                            &mut tree,
                            &node,
                            &renderer,
                            viewport,
                            controls.label("Show matching lines"),
                        );
                        assert!(
                            matches!(messages.as_slice(), [Message::AdvancedFindAllOpenRun] if open)
                                || matches!(messages.as_slice(), [Message::AdvancedFindAllCurrentRun] if !open)
                        );
                    }
                    snapshot(
                        &format!(
                            "advanced-count-{theme_name}-open-{open}-display-{display}-zero-{no_matches}"
                        ),
                        &element,
                        &tree,
                        &node,
                        &mut renderer,
                        Size::new(720, 560),
                        &theme,
                    );
                }
            }
        }
    }
}

#[test]
fn status_light_morph_stays_bright_and_inside_its_bounds() {
    use motion::StatusLightState::{Error, Idle, Searching, Success};
    const FRAME_COUNT: usize = 83;
    const SCALE: f32 = 8.0;
    const PIXELS: u32 = 96;
    let exporting = std::env::var_os("FRAGILE_SEARCH_SNAPSHOTS").is_some();
    let viewport = Rectangle::with_size(Size::new(12.0, 12.0));
    let limits = layout::Limits::new(Size::ZERO, viewport.size());
    let mut renderer = renderer();
    if exporting {
        std::fs::create_dir_all("target/search-review").expect("search review directory");
    }
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        let mut element = motion::status_light(Idle);
        let mut tree = Tree::empty();
        tree.diff(element.as_widget_mut());
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let start = Instant::now();
        for frame in 0..FRAME_COUNT {
            let state = match frame {
                0..=4 => Idle,
                5..=26 => Searching,
                27..=37 => Success,
                38..=40 => Searching,
                41..=43 => Idle,
                44..=59 => Searching,
                60..=70 => Error,
                _ => Idle,
            };
            element = motion::status_light(state);
            tree.diff(element.as_widget_mut());
            assert!(
                send(
                    &mut element,
                    &mut tree,
                    &node,
                    &renderer,
                    viewport,
                    Event::Window(iced::window::Event::RedrawRequested(
                        start + Duration::from_millis(frame as u64 * 33)
                    )),
                    mouse::Cursor::Unavailable,
                )
                .is_empty()
            );
            renderer::Renderer::reset(&mut renderer, viewport);
            element.as_widget().draw(
                &tree,
                &mut renderer,
                &theme,
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &viewport,
            );
            let pixels = renderer.screenshot(Size::new(PIXELS, PIXELS), SCALE, Color::TRANSPARENT);
            assert!(
                pixels.chunks_exact(4).any(|pixel| pixel[3] >= 250),
                "{theme_name} frame{frame}: the changing shape must retain a bright core"
            );
            for edge in 0..PIXELS as usize {
                for index in [
                    edge,
                    (PIXELS as usize - 1) * PIXELS as usize + edge,
                    edge * PIXELS as usize,
                    edge * PIXELS as usize + PIXELS as usize - 1,
                ] {
                    assert_eq!(
                        pixels[index * 4 + 3],
                        0,
                        "{theme_name} frame{frame}: geometry must remain inside its12px bounds"
                    );
                }
            }
            if frame == 26 {
                let center_pixel = ((PIXELS / 2 * PIXELS + PIXELS / 2) * 4) as usize;
                assert!(
                    pixels[center_pixel + 3] < 50,
                    "{theme_name}: the dot must open into a hollow arc"
                );
                assert!(
                    pixels.chunks_exact(4).any(|pixel| pixel[3] >= 250
                        && if theme_name == "dark" {
                            u16::from(pixel[1]) > u16::from(pixel[0]) + 80
                                && pixel[1].abs_diff(pixel[2]) < 5
                        } else {
                            u16::from(pixel[2]) > u16::from(pixel[1]) + 60
                                && u16::from(pixel[1]) > u16::from(pixel[0]) + 60
                        }),
                    "{theme_name}: the spinner must use the requested aqua/blue tint"
                );
            }
            if exporting {
                let background = if theme_name == "dark" {
                    Color::from_rgb8(29, 30, 32)
                } else {
                    Color::from_rgb8(247, 247, 247)
                };
                let pixels = renderer.screenshot(Size::new(PIXELS, PIXELS), SCALE, background);
                tiny_skia::Pixmap::from_vec(
                    pixels,
                    tiny_skia::IntSize::from_wh(PIXELS, PIXELS).unwrap(),
                )
                .expect("RGBA status frame")
                .save_png(format!(
                    "target/search-review/status-morph-{theme_name}-{frame}.png"
                ))
                .expect("write status frame");
            }
        }
    }
    if exporting {
        std::fs::write(
            "target/search-review/status-morph.html",
            STATUS_MORPH_PREVIEW,
        )
        .expect("write native animation preview");
    }
}

const STATUS_MORPH_PREVIEW: &str = r#"<!doctype html>
<html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Search status motion · native renderer</title>
<style>
body{margin:0;background:#111214;color:#e8e9eb;font:15px system-ui,sans-serif;padding:40px}
main{max-width:760px;margin:auto}h1{font-size:24px;font-weight:600}p{color:#adb0b6;line-height:1.6}
.panels{display:grid;grid-template-columns:1fr 1fr;gap:16px}.panel{padding:24px;border-radius:12px;background:#1d1e20;border:1px solid #35373c}
.light{background:#f7f7f7;color:#202123;border-color:#d2d4d8}.large{display:block;width:96px;height:96px;margin:24px auto}
.status{display:flex;gap:8px;align-items:center;font-size:12px}.native{width:12px;height:12px}.mode{margin:0;font-size:13px;font-weight:600}
button{background:#30343b;color:inherit;padding:8px 14px;border:1px solid #555961;border-radius:6px;cursor:pointer}
.controls{display:flex;align-items:center;gap:12px;margin-top:20px}#time{color:#adb0b6;font-variant-numeric:tabular-nums}
@media(max-width:560px){body{padding:20px}.panels{grid-template-columns:1fr}}
</style><main><h1>Dot → spinner → dot</h1>
<p>Frames from the native renderer. The larger view shows the shape; the status row shows its actual 12px size. Includes completion, rapid reversals, and an error.</p>
<div class="panels"><section class="panel"><h2 class="mode">Dark · aqua</h2><img class="large" id="dark-large" alt="Enlarged dark indicator"><div class="status"><img class="native" id="dark-native" alt=""><span class="label"></span></div></section>
<section class="panel light"><h2 class="mode">Light · blue</h2><img class="large" id="light-large" alt="Enlarged light indicator"><div class="status"><img class="native" id="light-native" alt=""><span class="label"></span></div></section></div>
<div class="controls"><button id="pause">Pause</button><button id="step">Next frame</button><span id="time"></span></div>
</main><script>
let frame=0,playing=true;
const label=i=>i<5?'Idle':i<27?'Searching…':i<38?'Matches found':i<41?'Searching…':i<44?'Idle':i<60?'Searching…':i<71?'Pattern error':'Idle';
const paths=Array.from({length:83},(_,i)=>['dark','light'].map(mode=>`status-morph-${mode}-${i}.png`));
paths.flat().forEach(src=>{const img=new Image();img.src=src});
function draw(){['dark','light'].forEach((mode,k)=>['large','native'].forEach(size=>document.getElementById(`${mode}-${size}`).src=paths[frame][k]));document.querySelectorAll('.label').forEach(el=>el.textContent=label(frame));document.getElementById('time').textContent=`${frame*33} ms`}
function pause(){playing=false;document.getElementById('pause').textContent='Play'}
document.getElementById('pause').onclick=()=>{playing=!playing;document.getElementById('pause').textContent=playing?'Pause':'Play'};
document.getElementById('step').onclick=()=>{pause();frame=(frame+1)%83;draw()};
draw();setInterval(()=>{if(playing){frame=(frame+1)%83;draw()}},33);
</script></html>"#;

#[test]
fn inline_search_keeps_controls_visible_at_the_workbench_minimum_width() {
    let mut find = FindState::with_query("needle");
    find.set_replacement("thread");
    find.refresh_matches("needle and another needle");
    let mut renderer = renderer();
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        for replace in [false, true] {
            let size = Size::new(640.0, if replace { 86.0 } else { 46.0 });
            let viewport = Rectangle::with_size(size);
            let mut element = find_panel::view(&find, replace, replace, 1.0);
            let (mut tree, node, controls) = settle(&mut element, &renderer, size);
            assert_eq!(node.size(), size);
            assert!(controls.input(find_panel::FIND_INPUT_ID).width >= 120.0);
            for (_, bounds) in &controls.inputs {
                visible(*bounds, viewport);
            }
            snapshot(
                &format!("inline-{theme_name}-replace-{replace}"),
                &element,
                &tree,
                &node,
                &mut renderer,
                Size::new(640, size.height as u32),
                &theme,
            );
            let find_row = Layout::new(&node).child(0).child(0);
            let actions = find_row.child(2);
            for (index, expected) in [(1, "previous"), (2, "next"), (3, "advanced"), (4, "close")] {
                let messages = click(
                    &mut element,
                    &mut tree,
                    &node,
                    &renderer,
                    viewport,
                    actions.child(index).bounds(),
                );
                assert!(
                    matches!(messages.as_slice(), [message] if match (expected, message) {
                        ("previous", Message::FindPrevious) | ("next", Message::FindNext) | ("close", Message::HideFind) => true,
                        ("advanced", Message::ToggleAdvancedSearch(tab)) => *tab == if replace { AdvancedSearchTab::Replace } else { AdvancedSearchTab::Find },
                        _ => false,
                    }),
                    "inline {expected}: {messages:?}"
                );
            }
            for label in ["Aa", "ab", ".*"] {
                let messages = click(
                    &mut element,
                    &mut tree,
                    &node,
                    &renderer,
                    viewport,
                    controls.label(label),
                );
                assert!(
                    matches!(messages.as_slice(), [message] if match (label, message) {
                        ("Aa", Message::FindCaseSensitiveToggled(true)) | ("ab", Message::FindWholeWordToggled(true)) => true,
                        (".*", Message::FindModeSelected(fragile_notepad::core::SearchMode::Regex)) => true,
                        _ => false,
                    }),
                    "inline {label}: {messages:?}"
                );
            }
            assert!(matches!(
                click(
                    &mut element,
                    &mut tree,
                    &node,
                    &renderer,
                    viewport,
                    find_row.child(0).bounds()
                )
                .as_slice(),
                [Message::ToggleInlineReplace]
            ));
            if replace {
                assert!(matches!(
                    click(
                        &mut element,
                        &mut tree,
                        &node,
                        &renderer,
                        viewport,
                        controls.label("Replace")
                    )
                    .as_slice(),
                    [Message::ReplaceCurrent]
                ));
                assert!(matches!(
                    click(
                        &mut element,
                        &mut tree,
                        &node,
                        &renderer,
                        viewport,
                        controls.label("Replace all")
                    )
                    .as_slice(),
                    [Message::ReplaceAll]
                ));
                assert!(
                    matches!(type_into(&mut element, &mut tree, &node, &renderer, viewport, find_panel::REPLACE_INPUT_ID).as_slice(), [Message::FindReplacementChanged(replacement)] if replacement == "x")
                );
            }
            assert!(
                matches!(type_into(&mut element, &mut tree, &node, &renderer, viewport, find_panel::FIND_INPUT_ID).as_slice(), [Message::FindQueryChanged(query)] if query == "x")
            );
        }
    }
}
