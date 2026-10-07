//! App-local editor model primitives.

pub mod action;
pub mod buffer;
pub mod cjk;
pub mod decoration;
pub mod delimiter;
pub mod fold;
pub mod history;
pub mod layout;
pub mod movement;
pub mod outline;
pub mod position;
pub mod render;
mod syntax_hints;
pub mod viewport;
pub mod widget;
mod word;
pub(crate) mod wrap_measurement;

pub use action::{CaretMotion, EditorAction};
pub use buffer::{EditDelta, EditorBuffer};
pub use decoration::{
    DecorationModel, DecorationSettings, HiddenLineSpan, IndentGuide, LineDecoration,
};
pub use delimiter::{DelimiterMatch, matching_delimiter_at, matching_delimiter_near_caret};
pub use fold::{FoldModel, FoldProvider, FoldRange, IndentBraceFoldProvider};
pub use history::{EditTransaction, EditorHistory};
pub use layout::{
    EditorLayout, EditorMetrics, HitTarget, ScrollOffset, caret_x, hit_test, hit_visible_row, row_y,
};
pub use movement::{
    document_end, is_vertical_motion, line_end, move_position, move_position_with_column,
    next_grapheme_offset, next_grapheme_position, previous_grapheme_offset,
    previous_grapheme_position,
};
pub use outline::{
    FunctionEntry, FunctionKind, OutlineParseResult, OutlineSnapshotMetadata, OutlineState,
    OutlineStatus, containing_function, next_function_after, outline_for_syntax,
    outline_registry_hash, outline_request_for_document, parse_outline_request,
    parse_outline_snapshot, previous_function_before,
};
pub use position::{
    EditorPosition, EditorRange, EditorSelection, ProjectedSelectionLine, RectangularSelection,
    SelectionRange, SelectionSet, SelectionShape, position_after_text, position_for_byte_offset,
};
pub use render::{
    CaretRenderPlan, EolRenderPlan, FoldRenderPlan, HiddenLineRenderPlan, IndentGuideRenderPlan,
    RenderPlan, RowRenderPlan, SelectionRenderPlan, SyntaxLineCache, SyntaxRenderSpan,
    WhitespaceKind, WhitespaceRenderPlan, build_render_plan,
    build_render_plan_for_selection_set_with_cache,
    build_render_plan_for_selection_set_with_cache_and_caret_row,
    build_render_plan_for_selection_set_with_cache_and_caret_rows, build_render_plan_with_cache,
    line_number_left_x, line_number_text_x, planned_text_draws, planned_text_draws_with_markers,
    space_marker_bounds, space_marker_size, text_baseline_offset, text_size,
    visible_marker_columns,
};
pub use viewport::{RowSegment, ViewportModel, VisibleRow};
pub use widget::{AdvancedEditor, AdvancedEditorState};
pub use word::word_range_at_position;
