//! Composition of the main workbench surface and its transient overlays.

mod background;
mod tab_context_menu;

use iced::widget::{Column, container, row, space, stack};
use iced::{Element, Fill, Length};

use crate::core::FindState;
use crate::message::Message;

use super::view_model::{ChromeAnimationInfo, WorkbenchView};
use super::{
    about_dialog, dirty_close_dialog, editor, find_panel, function_list_panel, motion,
    raster_cache, status_bar, styles, tab_list_dialog, tabs, toolbar, window_list_dialog,
};

const FIND_PANEL_COLLAPSED_HEIGHT: f32 = 46.0;
const FIND_PANEL_EXPANDED_HEIGHT: f32 = 86.0;

pub(crate) fn view<'a>(model: WorkbenchView<'a>) -> Element<'a, Message> {
    let WorkbenchView {
        workspace,
        editor_window,
        find,
        settings,
        is_find_visible,
        is_inline_replace_visible,
        is_function_list_visible,
        function_list_query,
        chrome_animation,
        active_menu,
        active_menu_path,
        window_menu_state,
        dragged_tab,
        tab_drag_position,
        hovered_drop_tab,
        dirty_close_document,
        about_tab,
        rendering_debug_info,
        window_list_entries,
        window_list_query,
        tab_list_query,
        file_status,
        active_outline_state,
    } = model;
    // These transitions are driven by App messages, outside child redraw
    // deadlines. Capture settled pixels instead of rebuilding surfaces per frame.
    let retain = [
        (chrome_animation.find_progress, is_find_visible),
        (
            chrome_animation.inline_replace_progress,
            is_inline_replace_visible,
        ),
        (
            chrome_animation.function_list_progress,
            is_function_list_visible,
        ),
        (
            chrome_animation.about_progress,
            chrome_animation.about_interactive,
        ),
        (
            chrome_animation.dirty_close_progress,
            chrome_animation.dirty_close_interactive,
        ),
        (
            chrome_animation.selector_progress,
            chrome_animation.selector_interactive,
        ),
    ]
    .into_iter()
    // Compare the target too: the first closing frame still has opacity 1,
    // and the first opening frame still has opacity 0.
    .all(|(progress, visible)| progress == if visible { 1.0 } else { 0.0 });
    let active_document = workspace.active_document_in(editor_window);
    let editor = if let Some(document) = active_document {
        // Wrap outside the context menu, whose child is the AdvancedEditor tree.
        raster_cache::cached_editor(background::editor(editor::view(document, settings)), retain)
    } else {
        editor::empty()
    };

    // from_vec preserves the Find slot at zero height. This lets its opening
    // focus operation reach the input and keeps the editor's tree index stable.
    let mut workbench = vec![
        if cfg!(target_os = "macos") {
            space::vertical().height(0.0).width(Fill).into()
        } else {
            raster_cache::cached(toolbar::menu_bar(active_menu), retain)
        },
        raster_cache::cached(toolbar::tool_bar(active_document), retain),
        tabs::view_with_drag(
            workspace,
            editor_window,
            dragged_tab,
            hovered_drop_tab,
            tab_drag_position,
        ),
        find_panel_slot(
            find,
            is_find_visible,
            is_inline_replace_visible,
            chrome_animation,
        ),
    ];

    let editor_surface = container(editor)
        .height(Fill)
        .width(Fill)
        .style(move |theme| {
            let mut style = styles::editor_surface(theme);
            // AdvancedEditor fills its bounds; retain the fill for the empty state.
            if active_document.is_some() {
                style.background = None;
            }
            style
        });

    let main_area: Element<'a, Message> = if chrome_animation.function_list_rendered_visible
        || is_function_list_visible
    {
        if let Some(document) = active_document {
            let function_list_width = function_list_panel::FUNCTION_LIST_PANEL_WIDTH
                * chrome_animation.function_list_progress.clamp(0.0, 1.0);

            row![
                editor_surface,
                container(motion::fade(
                    function_list_panel::view(document, active_outline_state, function_list_query),
                    chrome_animation.function_list_progress,
                    styles::editor_background,
                    is_function_list_visible,
                ))
                .width(Length::Fixed(function_list_width))
                .height(Fill)
                .clip(true),
            ]
            .height(Fill)
            .width(Fill)
            .into()
        } else {
            editor_surface.into()
        }
    } else {
        editor_surface.into()
    };

    workbench.push(main_area);
    workbench.push(raster_cache::cached(
        status_bar::view(active_document, settings, file_status),
        retain,
    ));
    let shell = container(background::shell(Column::from_vec(workbench)))
        .height(Fill)
        .width(Fill)
        .style(styles::app_shell);
    // A quiet shell needs one composite. When any child has a pending redraw
    // (caret, scrolling, or fade), draw it live and let the smaller caches help.
    let shell = raster_cache::cached_animated(shell, retain);
    // The popup owns input after every workbench child has updated, so the
    // editor cannot re-enable IME while it is open. Keep redraws uncaptured:
    // Iced removes rendered overlays when the base captures an event.
    let shell = tab_context_menu::wrap(
        shell,
        workspace,
        editor_window,
        settings,
        window_menu_state.editor_windows.clone(),
        dragged_tab.is_some(),
    );

    let with_menu: Element<'a, Message> = if !cfg!(target_os = "macos") && active_menu.is_some() {
        stack![
            shell,
            container(toolbar::menu_overlay(
                active_menu,
                active_menu_path,
                window_menu_state,
                settings,
                active_document
            ))
            .width(Fill)
            .height(Fill),
        ]
        .into()
    } else {
        shell
    };

    let with_dialogs = if let Some(document) = dirty_close_document {
        stack![
            motion::fade(with_menu, 1.0, styles::editor_background, false),
            dirty_close_dialog::view(
                document,
                chrome_animation.dirty_close_progress,
                chrome_animation.dirty_close_interactive,
            )
        ]
        .into()
    } else {
        with_menu
    };

    let with_window_list = if let Some(entries) = window_list_entries {
        stack![
            motion::fade(with_dialogs, 1.0, styles::editor_background, false),
            window_list_dialog::view_animated(
                entries,
                window_list_query,
                chrome_animation.selector_progress,
                chrome_animation.selector_interactive,
            )
        ]
        .into()
    } else if let Some(query) = tab_list_query {
        stack![
            motion::fade(with_dialogs, 1.0, styles::editor_background, false),
            tab_list_dialog::view_animated(
                workspace,
                editor_window,
                query,
                chrome_animation.selector_progress,
                chrome_animation.selector_interactive,
            )
        ]
        .into()
    } else {
        with_dialogs
    };

    if let Some(tab) = about_tab {
        stack![
            with_window_list,
            about_dialog::view(
                tab,
                rendering_debug_info,
                chrome_animation.about_progress,
                chrome_animation.about_interactive,
            )
        ]
        .into()
    } else {
        with_window_list
    }
}

fn find_panel_slot(
    find: &FindState,
    visible: bool,
    replace_visible: bool,
    animation: ChromeAnimationInfo,
) -> Element<'_, Message> {
    if !visible && !animation.find_rendered_visible {
        return space::vertical().height(0.0).width(Fill).into();
    }
    container(motion::fade(
        find_panel::view(
            find,
            replace_visible,
            animation.inline_replace_rendered_visible,
            animation.inline_replace_progress,
        ),
        animation.find_progress,
        styles::utility_bar_background,
        visible,
    ))
    .height(Length::Fixed(animated_find_height(animation)))
    .width(Fill)
    .clip(true)
    .into()
}

fn animated_find_height(animation: ChromeAnimationInfo) -> f32 {
    let find_progress = animation.find_progress.clamp(0.0, 1.0);
    let replace_progress = animation.inline_replace_progress.clamp(0.0, 1.0);
    let expanded_height = FIND_PANEL_COLLAPSED_HEIGHT
        + ((FIND_PANEL_EXPANDED_HEIGHT - FIND_PANEL_COLLAPSED_HEIGHT) * replace_progress);

    expanded_height * find_progress
}
