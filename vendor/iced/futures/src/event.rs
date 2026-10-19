//! Listen to runtime events.
use crate::MaybeSend;
use crate::core::event::{self, Event};
use crate::core::window;
use crate::subscription::{self, Subscription};

/// Returns a [`Subscription`] to all the ignored runtime events.
///
/// This subscription will notify your application of any [`Event`] that was
/// not captured by any widget.
pub fn listen() -> Subscription<Event> {
    listen_with(|event, status, _window| match status {
        event::Status::Ignored => Some(event),
        event::Status::Captured => None,
    })
}

/// Creates a [`Subscription`] that listens and filters all the runtime events
/// with the provided function, producing messages accordingly.
///
/// This subscription will call the provided function for every [`Event`]
/// handled by the runtime. If the function:
///
/// - Returns `None`, the [`Event`] will be discarded.
/// - Returns `Some` message, the `Message` will be produced.
pub fn listen_with<Message>(
    f: fn(Event, event::Status, window::Id) -> Option<Message>,
) -> Subscription<Message>
where
    Message: 'static + MaybeSend,
{
    #[derive(Hash)]
    struct EventsWith;

    subscription::filter_map((EventsWith, f), move |event| match event {
        subscription::Event::Interaction {
            event: Event::Window(window::Event::RedrawRequested(_)),
            ..
        }
        | subscription::Event::SystemThemeChanged(_)
        | subscription::Event::PlatformSpecific(_) => None,
        subscription::Event::Interaction {
            window,
            event,
            status,
        } => f(event, status, window),
    })
}

/// Creates a [`Subscription`] that produces a message for every runtime event,
/// including the redraw request events.
///
/// **Warning:** This [`Subscription`], if unfiltered, may produce messages in
/// an infinite loop.
pub fn listen_raw<Message>(
    f: fn(Event, event::Status, window::Id) -> Option<Message>,
) -> Subscription<Message>
where
    Message: 'static + MaybeSend,
{
    #[derive(Hash)]
    struct RawEvents;

    subscription::filter_map((RawEvents, f), move |event| match event {
        subscription::Event::Interaction {
            window,
            event,
            status,
        } => f(event, status, window),
        subscription::Event::SystemThemeChanged(_) | subscription::Event::PlatformSpecific(_) => {
            None
        }
    })
}

/// Creates a [`Subscription`] that notifies of custom application URL
/// received from the system.
///
/// _**Note:** Currently, it only triggers on macOS and the executable needs to be properly [bundled]!_
///
/// [bundled]: https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html#//apple_ref/doc/uid/10000123i-CH101-SW19
pub fn listen_url() -> Subscription<String> {
    #[derive(Hash)]
    struct ListenUrl;

    subscription::filter_map(ListenUrl, move |event| match event {
        subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(
            subscription::MacOS::ReceivedUrl(url),
        )) => Some(url),
        _ => None,
    })
}

/// Notifies when macOS requests application termination through its menu or Dock.
/// A daemon can save its state or ask for confirmation, then explicitly exit
/// the runtime when it is ready to quit.
pub fn listen_quit() -> Subscription<()> {
    #[derive(Hash)]
    struct ListenQuit;
    subscription::filter_map(ListenQuit, |event| match event {
        subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(
            subscription::MacOS::QuitRequested,
        )) => Some(()),
        _ => None,
    })
}

/// Notifies when a running macOS application is reopened from its Dock icon.
/// A daemon can use this to create a new editor after its last window closed.
pub fn listen_reopen() -> Subscription<()> {
    #[derive(Hash)]
    struct ListenReopen;
    subscription::filter_map(ListenReopen, |event| match event {
        subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(
            subscription::MacOS::Reopened,
        )) => Some(()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, StreamExt, stream};

    #[test]
    fn quit_subscription_filters_other_events_and_keeps_repeated_requests() {
        let input = [
            subscription::MacOS::Reopened,
            subscription::MacOS::QuitRequested,
            subscription::MacOS::ReceivedUrl("fragile://example".into()),
            subscription::MacOS::QuitRequested,
        ]
        .map(|event| {
            subscription::Event::PlatformSpecific(subscription::PlatformSpecific::MacOS(event))
        });
        let recipe = subscription::into_recipes(listen_quit()).pop().unwrap();
        let mut output = recipe.stream(crate::boxed_stream(stream::iter(input)));

        assert_eq!(output.next().now_or_never(), Some(Some(())));
        assert_eq!(output.next().now_or_never(), Some(Some(())));
        assert_eq!(output.next().now_or_never(), Some(None));
    }
}
