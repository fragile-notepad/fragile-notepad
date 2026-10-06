//! Runtime event sources and single-instance admission.

use super::{App, shortcuts};
use crate::ipc::{PrimaryInstance, Signal};
use crate::message::Message;
use iced::{Subscription, event, stream, window};
use std::sync::OnceLock;

static SINGLE_INSTANCE: OnceLock<PrimaryInstance> = OnceLock::new();

impl App {
    pub(super) fn subscriptions(&self) -> Subscription<Message> {
        let chrome_animation = if self.needs_animation_frames() {
            window::frames().map(Message::ChromeAnimationFrame)
        } else {
            Subscription::none()
        };

        Subscription::batch([
            single_instance_subscription(),
            iced::system::theme_changes().map(Message::SystemColorModeChanged),
            event::listen_with(shortcuts::event_to_message),
            window::close_requests().map(Message::WindowCloseRequested),
            window::close_events().map(Message::WindowClosed),
            chrome_animation,
        ])
    }
}

pub fn register_single_instance(instance: PrimaryInstance) {
    let _ = SINGLE_INSTANCE.set(instance);
}

fn single_instance_subscription() -> Subscription<Message> {
    SINGLE_INSTANCE
        .get()
        .filter(|instance| instance.supports_signals())
        .map(|_| Subscription::run(single_instance_signals))
        .unwrap_or_else(Subscription::none)
}

fn single_instance_signals() -> impl iced::futures::Stream<Item = Message> {
    stream::channel(8, async |mut output| {
        let Some(instance) = SINGLE_INSTANCE.get() else {
            return;
        };

        std::thread::spawn(move || {
            loop {
                let mut disconnected = false;
                let accepted = instance.accept_signal_with(|signal| {
                    use iced::futures::SinkExt;
                    let receipt = crate::ipc::AdmissionReceipt::new();
                    let (paths, request) = match signal {
                        Signal::Show(request) => (Vec::new(), request.clone()),
                        Signal::OpenFiles(paths, request) => (paths.clone(), request.clone()),
                    };
                    if futures::executor::block_on(output.send(Message::ForwardedFiles(
                        paths,
                        request,
                        receipt.clone(),
                    )))
                    .is_err()
                    {
                        disconnected = true;
                        return false;
                    }
                    receipt.wait_for_acceptance()
                });
                if disconnected {
                    break;
                }
                match accepted {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_error) => break,
                }
            }
        });

        std::future::pending::<()>().await;
    })
}
