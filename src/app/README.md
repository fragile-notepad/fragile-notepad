[Documentation](../../docs/README.md)

# Application source overview

This module coordinates editor workflows, task cancellation, native windows,
and event reactions. Start with [Application architecture](../../docs/architecture/application.md)
for ownership rules, update flow, window routing, tab transfers, persistence,
and shutdown behavior.

## Entry points

| Responsibility | Source |
| --- | --- |
| `App` state and Iced entry points | [../app.rs](../app.rs) |
| Bootstrap and message routing | [bootstrap.rs](bootstrap.rs), [routing.rs](routing.rs) |
| Subscriber wiring and reaction dependencies | [update.rs](update.rs), [scheduler.rs](scheduler.rs) |
| Typed events and queued delivery | [events.rs](events.rs), [bus.rs](bus.rs) |
| Native windows and window-local state | [windowing/mod.rs](windowing/mod.rs), [windowing/editors.rs](windowing/editors.rs) |
| Tab drag and transfer transactions | [windowing/tabs.rs](windowing/tabs.rs) |
| File, session, and shutdown workflows | [files.rs](files.rs), [session.rs](session.rs), [lifecycle.rs](lifecycle.rs) |
| Borrowed UI presentation | [presentation.rs](presentation.rs), [../ui/view_model.rs](../ui/view_model.rs) |
| Message routing and shutdown-delivery declarations | [../message.rs](../message.rs) |

## Related documentation

- [Rendering architecture](../../docs/architecture/rendering.md)
- [Syntax configuration reference](../../assets/syntax/README.md)
- [Testing](../../docs/development/testing.md)
