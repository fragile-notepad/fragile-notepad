use iced::widget::{button, column, container, row, scrollable, space, text, text_input, tooltip};
use iced::{Center, Element, Fill, Font};

use crate::core::{Document, DocumentLoadState};
use crate::editor::outline::{OutlineNode, OutlineNodeKind};
use crate::editor::{EditorRange, FunctionKind, OutlineState, OutlineStatus};
use crate::message::Message;
use crate::ui::controls::centered_button_content;
use crate::ui::icons::hero::{self, HeroIcon, IconTone};
use crate::ui::styles;

pub const FUNCTION_LIST_PANEL_WIDTH: f32 = 280.0;
pub const FUNCTION_LIST_PANEL_TITLE: &str = "Function List";
pub const FUNCTION_LIST_EMPTY_MESSAGE: &str = "No symbols found";
pub const FUNCTION_LIST_PENDING_MESSAGE: &str = "Scanning symbols…";
pub const FUNCTION_LIST_UNAVAILABLE_MESSAGE: &str = "Symbols unavailable";
pub const SCROLL_ID: &str = "function-list-scroll";
pub const INPUT_ID: &str = "function-list-filter";

pub fn view<'a>(
    document: &'a Document,
    outline_state: Option<&'a OutlineState>,
    query: &'a str,
) -> Element<'a, Message> {
    let ready = outline_state.filter(|state| state.status == OutlineStatus::Ready);
    let visible = ready.map_or_else(Vec::new, |state| visible_rows(state, query));
    let unfiltered = query.trim().is_empty();
    let total = if unfiltered {
        visible.len()
    } else {
        ready.map_or(0, |state| {
            if state.tree.roots.is_empty() {
                state.functions.len()
            } else {
                symbol_count(&state.tree.roots)
            }
        })
    };
    let caret = document.main_selection().cursor;
    let current = visible
        .iter()
        .filter(|row| row.range.start <= caret && caret < row.range.end)
        .max_by_key(|row| (row.depth, row.range.start))
        .map(|row| row.range);
    let count = if ready.is_none() {
        String::from("—")
    } else if unfiltered {
        total.to_string()
    } else {
        format!(
            "{} / {}",
            visible.iter().filter(|row| row.matched).count(),
            total
        )
    };

    let header = row![
        // Let the title absorb font-dependent overflow after the count and
        // close button have received their space. A trailing fill spacer cannot
        // stop an intrinsically sized title from squeezing the button to zero.
        text(FUNCTION_LIST_PANEL_TITLE)
            .size(13)
            .font(Font {
                weight: iced::font::Weight::Semibold,
                ..Font::DEFAULT
            })
            .width(Fill)
            .wrapping(text::Wrapping::None)
            .ellipsis(text::Ellipsis::End),
        container(text(count).size(11))
            .padding([2, 6])
            .style(styles::function_list_count),
        icon_button(
            HeroIcon::XMark,
            "Close function list",
            Message::ToggleFunctionList
        ),
    ]
    .width(Fill)
    .spacing(7)
    .align_y(Center);

    let title = document.title();
    let file = tooltip(
        text(title.clone())
            .size(12)
            .width(Fill)
            .wrapping(text::Wrapping::None)
            .ellipsis(text::Ellipsis::Middle)
            .style(styles::function_list_secondary),
        container(text(title).size(12))
            .padding(6)
            .style(styles::tooltip),
        tooltip::Position::Bottom,
    );

    let mut input = text_input("Filter symbols…", query)
        .id(INPUT_ID)
        .on_input(Message::FunctionListQueryChanged)
        .size(12)
        .padding([7, 9])
        .width(Fill)
        .style(styles::input);
    if let Some(first) = visible.iter().find(|row| row.matched) {
        input = input.on_submit(Message::FunctionListEntrySelected(first.range.start));
    }
    let mut filter = row![input].spacing(4).align_y(Center);
    if !query.is_empty() {
        filter = filter.push(icon_button(
            HeroIcon::XMark,
            "Clear filter",
            Message::FunctionListQueryChanged(String::new()),
        ));
    }

    let body = if ready.is_none() {
        let (title, detail) = unavailable_or_pending_message(document, outline_state);
        empty_state(title, detail)
    } else if total == 0 {
        empty_state(
            FUNCTION_LIST_EMPTY_MESSAGE,
            "Functions and types in this file will appear here.",
        )
    } else if visible.is_empty() {
        empty_state(
            "No matching symbols",
            "Try another name or clear the filter.",
        )
    } else {
        visible
            .into_iter()
            .fold(column![].spacing(2).padding([4, 6]), |rows, symbol| {
                let active = current == Some(symbol.range);
                rows.push(symbol_row(symbol, active))
            })
            .into()
    };

    container(
        column![
            container(column![header, file, filter].spacing(8))
                .padding([10, 12])
                .width(Fill)
                .style(styles::function_list_header),
            scrollable(body)
                .style(styles::scrollable)
                .id(SCROLL_ID)
                .smooth_scroll(true)
                .height(Fill),
        ]
        .height(Fill),
    )
    .width(FUNCTION_LIST_PANEL_WIDTH)
    .height(Fill)
    .style(styles::function_list_panel)
    .into()
}

fn unavailable_or_pending_message(
    document: &Document,
    outline_state: Option<&OutlineState>,
) -> (&'static str, &'static str) {
    if outline_state.is_some_and(|state| state.status == OutlineStatus::Unavailable) {
        if matches!(document.load_state, DocumentLoadState::Failed { .. }) {
            (
                FUNCTION_LIST_UNAVAILABLE_MESSAGE,
                "Reload this file to build its symbol list.",
            )
        } else {
            (
                FUNCTION_LIST_UNAVAILABLE_MESSAGE,
                "Symbol scanning is available for files up to 1 MiB.",
            )
        }
    } else if !document.has_complete_text_index() {
        (
            "Waiting for file contents…",
            "Symbols will be scanned when the file finishes loading.",
        )
    } else {
        (
            FUNCTION_LIST_PENDING_MESSAGE,
            "The list updates as you edit.",
        )
    }
}

fn symbol_count(nodes: &[OutlineNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + symbol_count(&node.children))
        .sum()
}

#[derive(Debug)]
struct SymbolRow<'a> {
    name: &'a str,
    kind: OutlineNodeKind,
    range: EditorRange,
    depth: usize,
    matched: bool,
}

fn visible_rows<'a>(state: &'a OutlineState, query: &str) -> Vec<SymbolRow<'a>> {
    let query = query.trim().to_lowercase();
    // Function-only snapshots are also used by callers without an outline tree.
    if state.tree.roots.is_empty() {
        return state
            .functions
            .iter()
            .filter(|entry| query.is_empty() || entry.name.to_lowercase().contains(&query))
            .map(|entry| SymbolRow {
                name: &entry.name,
                kind: match entry.kind {
                    FunctionKind::Function => OutlineNodeKind::Function,
                    FunctionKind::Method => OutlineNodeKind::Method,
                    FunctionKind::Declaration => OutlineNodeKind::Declaration,
                },
                range: entry.range,
                depth: entry.depth,
                matched: true,
            })
            .collect();
    }

    let mut rows = Vec::new();
    append_nodes(&state.tree.roots, &query, false, &mut rows);
    rows
}

fn append_nodes<'a>(
    nodes: &'a [OutlineNode],
    query: &str,
    parent_matches: bool,
    rows: &mut Vec<SymbolRow<'a>>,
) {
    // Caller-provided snapshots may store siblings in a different order.
    let mut ordered = nodes.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|node| node.range.start);
    for node in ordered {
        let start = rows.len();
        let name = if node.name.is_empty() {
            "(anonymous)"
        } else {
            &node.name
        };
        let matched = parent_matches || query.is_empty() || name.to_lowercase().contains(query);
        rows.push(SymbolRow {
            name,
            kind: node.kind,
            range: node.range,
            depth: node.depth,
            matched,
        });
        append_nodes(&node.children, query, matched, rows);
        // Keep ancestors as context; matching a container also shows its children.
        if rows.len() == start + 1 && !matched {
            rows.pop();
        }
    }
}

fn symbol_row(symbol: SymbolRow<'_>, active: bool) -> Element<'_, Message> {
    let (badge, kind) = match symbol.kind {
        OutlineNodeKind::Function => ("fn", "Function"),
        OutlineNodeKind::Method => ("m", "Method"),
        OutlineNodeKind::Constructor => ("ctor", "Constructor"),
        OutlineNodeKind::Declaration => ("d", "Declaration"),
        OutlineNodeKind::Module => ("mod", "Module"),
        OutlineNodeKind::Namespace => ("ns", "Namespace"),
        OutlineNodeKind::Class => ("cls", "Class"),
        OutlineNodeKind::Enum => ("enum", "Enum"),
        OutlineNodeKind::EnumMember => ("val", "Enum member"),
        OutlineNodeKind::Interface => ("ifc", "Interface"),
        OutlineNodeKind::Trait => ("tr", "Trait"),
        OutlineNodeKind::Impl => ("impl", "Implementation"),
        OutlineNodeKind::Tag => ("tag", "Tag"),
        OutlineNodeKind::Section => ("sec", "Section"),
        OutlineNodeKind::Unknown => ("…", "Container"),
    };
    let label = container(text(badge).size(10).font(Font::MONOSPACE))
        .center_x(30)
        .center_y(22)
        .style(styles::function_list_kind_label(symbol.kind));
    let content = button(
        row![
            space::horizontal().width((symbol.depth.min(4) * 10) as f32),
            label,
            text(symbol.name)
                .size(13)
                .font(Font {
                    weight: if matches!(
                        symbol.kind,
                        OutlineNodeKind::Function
                            | OutlineNodeKind::Method
                            | OutlineNodeKind::Constructor
                            | OutlineNodeKind::Declaration
                            | OutlineNodeKind::EnumMember
                    ) {
                        iced::font::Weight::Normal
                    } else {
                        iced::font::Weight::Semibold
                    },
                    ..Font::DEFAULT
                })
                .width(Fill)
                .wrapping(text::Wrapping::None)
                .ellipsis(text::Ellipsis::End),
            text((symbol.range.start.line + 1).to_string())
                .size(11)
                .style(styles::function_list_secondary),
        ]
        .spacing(7)
        .align_y(Center)
        .width(Fill),
    )
    .padding([5, 7])
    .width(Fill)
    .style(styles::function_list_entry(active))
    .on_press(Message::FunctionListEntrySelected(symbol.range.start));

    tooltip(
        content,
        container(
            column![
                text(symbol.name).size(13),
                text(format!("{kind} · Line {}", symbol.range.start.line + 1)).size(11),
            ]
            .spacing(4),
        )
        .padding(8)
        .max_width(420)
        .style(styles::tooltip),
        tooltip::Position::Left,
    )
    .gap(8)
    .into()
}

fn icon_button(icon: HeroIcon, label: &'static str, message: Message) -> Element<'static, Message> {
    tooltip(
        button(centered_button_content(hero::icon(
            icon,
            14,
            IconTone::Muted,
        )))
        .width(24)
        .height(24)
        .padding(0)
        .style(styles::icon_button)
        .on_press(message),
        container(text(label).size(12))
            .padding([4, 7])
            .style(styles::tooltip),
        tooltip::Position::Bottom,
    )
    .into()
}

fn empty_state(title: &'static str, detail: &'static str) -> Element<'static, Message> {
    container(
        column![
            container(text("fn").size(18).font(Font::MONOSPACE))
                .center_x(40)
                .center_y(36)
                .style(styles::function_list_count),
            text(title).size(13),
            text(detail).size(12).style(styles::function_list_secondary),
        ]
        .spacing(10),
    )
    .padding([28, 18])
    .width(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorBuffer;
    use crate::editor::outline::{OutlineEngine, OutlineRegistry};

    fn parse(source: &str, syntax: &str) -> OutlineState {
        let registry = OutlineRegistry::shared();
        OutlineState::ready(
            OutlineEngine::new(
                registry.plan_for_syntax(syntax).unwrap(),
                registry.registry_hash(),
            )
            .parse_buffer(&EditorBuffer::from_text(source), syntax),
        )
    }

    #[test]
    fn xml_parsed_parent_context_is_retained_across_languages() {
        for (syntax, source, expected) in [
            (
                "rs",
                "mod workspace { impl App { fn update() {} } }",
                vec!["workspace", "App", "update"],
            ),
            (
                "py",
                "class App:\n    def update(self):\n        pass",
                vec!["App", "update"],
            ),
            ("js", "class App { update() {} }", vec!["App", "update"]),
            (
                "ts",
                "class App { update(): void {} }",
                vec!["App", "update"],
            ),
            (
                "java",
                "class App { void update() {} }",
                vec!["App", "update"],
            ),
            ("kt", "class App { fun update() {} }", vec!["App", "update"]),
            (
                "cpp",
                "namespace workspace { class App { void update() {} }; }",
                vec!["workspace", "App", "update"],
            ),
            (
                "rb",
                "module Workspace\n  class App\n    def update\n    end\n  end\nend",
                vec!["Workspace", "App", "update"],
            ),
        ] {
            let state = parse(source, syntax);
            let rows = visible_rows(&state, "UPDATE");
            assert_eq!(
                rows.iter().map(|row| row.name).collect::<Vec<_>>(),
                expected,
                "{syntax}"
            );
            assert_eq!(
                rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
                (0..rows.len()).collect::<Vec<_>>(),
                "{syntax}"
            );
        }
    }

    #[test]
    fn rows_follow_source_order_and_filter_preserves_enclosing_functions() {
        let state = parse(
            "fn before() {}\nimpl App { fn outer() { fn leaf() {} } fn other() {} }\nfn after() {}",
            "rs",
        );
        let all = visible_rows(&state, "");
        assert_eq!(
            all.iter().map(|row| row.name).collect::<Vec<_>>(),
            ["before", "App", "outer", "leaf", "other", "after"]
        );
        let filtered = visible_rows(&state, "leaf");
        assert_eq!(
            filtered.iter().map(|row| row.name).collect::<Vec<_>>(),
            ["App", "outer", "leaf"]
        );
        assert!(visible_rows(&state, "missing").is_empty());
    }

    #[test]
    fn filtering_preserves_containers_declared_inside_functions() {
        for (syntax, source, expected) in [
            (
                "rs",
                "fn outer() { mod local { fn leaf() {} } fn sibling() {} }",
                vec!["outer", "local", "leaf"],
            ),
            (
                "py",
                "def outer():\n    class Local:\n        def leaf(self):\n            pass\n    def sibling():\n        pass\n",
                vec!["outer", "Local", "leaf"],
            ),
            (
                "js",
                "function outer() { class Local { leaf() {} } function sibling() {} }",
                vec!["outer", "Local", "leaf"],
            ),
        ] {
            let state = parse(source, syntax);
            let filtered = visible_rows(&state, "leaf");
            assert_eq!(
                filtered.iter().map(|row| row.name).collect::<Vec<_>>(),
                expected,
                "{syntax}"
            );
            assert_eq!(
                filtered.iter().map(|row| row.depth).collect::<Vec<_>>(),
                [0, 1, 2],
                "{syntax}"
            );
            assert!(!filtered[0].matched);
            assert!(!filtered[1].matched);
            assert!(filtered[2].matched);
        }
    }

    #[test]
    fn empty_messages_distinguish_loading_scanning_and_unavailable_files() {
        let mut document = Document::from_path(crate::core::DocumentId::new(1), "app.rs", "");
        let metadata = crate::editor::OutlineSnapshotMetadata::from_document(&document, 0);
        let pending = OutlineState::pending_metadata(metadata.clone());
        let unavailable = OutlineState::unavailable_metadata(metadata);
        assert_eq!(
            unavailable_or_pending_message(&document, Some(&pending)).0,
            FUNCTION_LIST_PENDING_MESSAGE
        );
        let (title, detail) = unavailable_or_pending_message(&document, Some(&unavailable));
        assert_eq!(title, FUNCTION_LIST_UNAVAILABLE_MESSAGE);
        assert!(detail.contains("1 MiB"));

        document.load_state = DocumentLoadState::Loading {
            generation: crate::core::DocumentLoadGeneration::next(),
            bytes_read: 0,
            total_bytes: None,
        };
        assert_eq!(
            unavailable_or_pending_message(&document, Some(&pending)).0,
            "Waiting for file contents…"
        );
        document.load_state = DocumentLoadState::Failed {
            generation: crate::core::DocumentLoadGeneration::next(),
        };
        assert!(
            unavailable_or_pending_message(&document, Some(&unavailable))
                .1
                .starts_with("Reload")
        );
    }
}
