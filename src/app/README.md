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

## Scheduling and shutdown

`bus.rs` preserves lifecycle events and coalesces invalidations only while queued;
remove keys before delivery to permit republication. Events carry IDs and facts.

Reaction order: files → search → workers/session persistence; analysis → syntax.
Save-then-close is explicitly sequenced.

Shutdown rejects new commands and IPC admission. Background messages queue
losslessly: successful exit discards them; failed exit replays them through separate
`App::update` boundaries before reactions resume. Declare delivery policy for new
async messages.
