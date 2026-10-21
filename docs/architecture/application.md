[Documentation](../README.md)

# Application architecture

`App` routes commands and coordinates editor workflows across native windows.

## Responsibilities

| Owner | Responsibilities |
| --- | --- |
| `App` | Workflow coordination, command routing, cancellation, and native-window state |
| `Workspace` | Live documents, document identity, editor-window membership, and mutation journal |
| File workflows | Load, save, and close queues |
| Session workflow | Persistence and recovery |
| Analysis features | Background workers and stale-result checks |
| `WorkbenchView` and widgets | Borrowed presentation state and user commands |
| Services | Typed results for external operations |

Subscribers receive their feature state and required inputs. They do not receive
`&mut App` or call each other.

## Update flow

1. `App::update` applies the message's admission policy and resolves its editor
   window.
2. Command routing dispatches the message to its feature handler.
3. The application publishes workspace and settings changes to the event bus.
4. The event bus delivers mutation events in FIFO order. Subscribers mark
   dependent work as pending.
5. Reactions run in dependency order until synchronous work settles. Scheduled
   asynchronous tasks return completion messages through later updates.

Asynchronous completion order is independent of the order in which work was
prepared. Features must check whether results still apply before using them.

## Document ownership and identity

`Workspace` owns every live `Document` and allocates document IDs process-wide.
Each core `EditorWindowId` owns ordered tab IDs, an active document, selected
tabs, and a selection anchor. A document belongs to exactly one editor window.

Transfers change membership and order without replacing document objects or
emitting document-opened or document-closed events. This preserves history,
views, and load generations. `documents()` enumerates the process registry;
`document_ids()` enumerates the current editor's tabs. Bulk commands must use an
explicit scope.

At update boundaries, the mutation journal compares metadata snapshots for
documents borrowed mutably and emits the corresponding change events. Use
document editing and setter methods to maintain revisions and immutable IDs.
Structural reorderings rebuild the ID index. Record intentional settings edits
at their mutation sites.

## Editor-window state and routing

`App.editor_windows` maps core IDs to native handles, placement, and `WindowUi`.
At an update boundary, `switch_editor` exchanges the current `WindowUi` with the
destination window's saved state:

- The active window's UI fields are staged directly on `App`; its registry entry
  has `ui: None`.
- Other windows retain `Some(WindowUi)` in their registry entries.

This permits feature handlers to borrow disjoint `App` fields while keeping find
state, menus, prompts, panels, and animations independent for each editor.
Application handlers must use `switch_editor` when activating another editor so
that the workspace context and staged UI state change together.

Every view emits `ForWindow(native_id, message)`. Document-addressed completion
messages resolve the document's current owner. Work started before a transfer
therefore applies to the same live document afterward. Returned widget
operations are scoped to their native window.

Presentation borrows each window's saved or staged state without changing the
active update context. Settings, file queues, the search utility window, worker
limits, and persistence remain process-wide. Visible editors share bounded
worker slots; a completion makes the next editor eligible for analysis, syntax,
or outline work.

## Tab-transfer transactions

Tab gestures are reversible layout transactions. Native events carry a gesture
session identity and track a held pointer across surfaces. Live transfers remain
provisional until release.

Escape or capture failure restores memberships, selection, and placement while
preserving current document contents. Empty source windows remain alive through
the gesture. Late callbacks must check the session identity and physical release
before moving a newly created native window.

## Sessions and close operations

Session version 2 stores one document registry together with window membership,
active tabs, placement, and the last active editor. Version 1 restores into one
editor. During dragging, snapshots retain the original layout together with
current recovery text.

Recovery captures Rope snapshots and prepares text on a worker. One writer
preserves snapshot order. See the [session file reference](../reference/runtime-options.md#settings-and-session-files)
for storage layout and limits.

Explicit close operations capture document IDs before prompts or saves. Later
focus and selection changes cannot redirect the operation. The final-window
exit setting defaults to enabled; disabling it permits a daemon with no editor
windows.

## Scheduling and shutdown

The event bus preserves lifecycle events and coalesces invalidations only while
they are queued. It removes coalescing keys before delivery so subscribers can
publish a fresh invalidation during handling. Document-specific events carry the
affected document's ID.

Reaction dependencies place file work before search, search before workers and
session persistence, and analysis before syntax. Save-then-close operations are
explicitly sequenced.

Shutdown rejects new commands and IPC admission. Background messages are queued
losslessly. A successful exit discards them; a failed exit resumes admission and
replays them through separate `App::update` boundaries. Declare the shutdown
delivery policy for every new asynchronous message.

## Source navigation

| Area | Entry points |
| --- | --- |
| Application state and bootstrap | [app.rs](../../src/app.rs), [bootstrap.rs](../../src/app/bootstrap.rs) |
| Command dispatch and presentation | [routing.rs](../../src/app/routing.rs), [presentation.rs](../../src/app/presentation.rs), [view_model.rs](../../src/ui/view_model.rs) |
| Events and reactions | [events.rs](../../src/app/events.rs), [bus.rs](../../src/app/bus.rs), [update.rs](../../src/app/update.rs), [scheduler.rs](../../src/app/scheduler.rs) |
| Window-local state and tab transfers | [windowing/mod.rs](../../src/app/windowing/mod.rs), [editors.rs](../../src/app/windowing/editors.rs), [tabs.rs](../../src/app/windowing/tabs.rs) |
| File and session workflows | [files.rs](../../src/app/files.rs), [session.rs](../../src/app/session.rs) |
| Admission and shutdown | [lifecycle.rs](../../src/app/lifecycle.rs), [message.rs](../../src/message.rs) |
