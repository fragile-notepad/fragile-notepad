# Application routing

`App` owns workflows and task cancellation; services return typed results.
`WorkbenchView` borrows state for UI widgets, which emit commands. `message.rs`
declares feature routing and shutdown policy; `update.rs` wires subscribers.

Updates dispatch a command, deliver mutation events FIFO, and run reactions until
synchronous work settles. Async completion order is independent of preparation order.

## Ownership

Workspace operations journal changes; document access compares scalar stamps at
update boundaries. Use document editing/setter methods to maintain revisions and
immutable IDs. Structural reorderings rebuild the ID index.

File workflows own load/save/close queues; sessions own persistence and recovery;
analysis features own workers and stale-result checks. Subscribers receive feature
state and required inputs, never `&mut App`.

Record intentional settings edits at mutation sites; session persistence observes
workspace changes.

## Editor windows

`Workspace` owns every live `Document` and allocates document IDs process-wide.
Each core `EditorWindowId` owns ordered tab IDs, an active document, selected
tabs, and a selection anchor. A document belongs to exactly one editor window.
Transfers change membership and order without replacing document objects or
emitting document-opened/closed events, preserving history, views, and load
generations. `documents()` enumerates the process registry; `document_ids()`
enumerates the current editor's tabs. Use an explicit scope for bulk commands.

`App.editor_windows` maps core IDs to native handles, placement, and `WindowUi`.
At an update boundary, `switch_editor` exchanges the current `WindowUi` with
the destination window's saved state. The active window's UI fields are staged
directly on `App`; its registry entry has `ui: None`. Other entries retain
`Some(WindowUi)`. This preserves feature handlers' disjoint field borrows while
keeping find state, menus, prompts, panels, and animations independent.
Application handlers must call `switch_editor`, rather than changing the core
workspace context directly, when activating another editor.

Every view emits `ForWindow(native_id, message)`. Document-addressed completion
messages resolve their document's current owner, so work started before a
transfer applies to the same live document afterward. Returned widget operations
are scoped to their native window. Presentation borrows each window's saved or
staged state without changing the active update context. Settings, file queues,
the search utility window, worker limits, and persistence remain process-wide.
Visible editors share bounded worker slots; a completion makes the next editor
eligible for analysis, syntax, or outline work.

Tab gestures are reversible layout transactions. Native session-tagged events
track a held pointer across surfaces; live transfers are provisional until
release. Escape or capture failure restores memberships, selection, and
placement while preserving current document contents. Empty source windows are
retained through the gesture. Late callbacks must check the session identity
and physical release before moving a newly created native window.

Session version 2 stores one document registry plus window membership, active
tabs, placement, and the last active editor. Version 1 restores into one editor.
During dragging, snapshots retain the original layout with current recovery
text. Explicit close operations capture document IDs before prompts or saves;
later focus and selection changes cannot redirect them. The final-window exit
setting defaults to enabled; disabling it permits a daemon with no editors.

## Scheduling and shutdown

`bus.rs` preserves lifecycle events and coalesces invalidations only while queued;
remove keys before delivery to permit republication. Events carry IDs and facts.

Reaction order: files → search → workers/session persistence; analysis → syntax.
Save-then-close is explicitly sequenced.

Shutdown rejects new commands and IPC admission. Background messages queue
losslessly: successful exit discards them; failed exit replays them through separate
`App::update` boundaries before reactions resume. Declare delivery policy for new
async messages.
