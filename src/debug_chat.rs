use std::lazy::SyncOnceCell;

use teloxide::{
    prelude::{Request, Requester},
    types::ChatId,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tracing::{info, warn};

use crate::{BotType, Config};

static DEBUG_CHANNEL: SyncOnceCell<Option<UnboundedSender<String>>> = SyncOnceCell::new();

#[must_use]
pub fn init<'a>(bot: BotType) -> Option<&'a UnboundedSender<String>> {
    DEBUG_CHANNEL
        .get_or_init(|| {
            match Config::get().debug_chat.map(|id| {
                let (tx, mut rx) = unbounded_channel();

                tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        if let Err(e) = bot.send_message(ChatId::Id(id), msg).send().await {
                            warn!("Failed to send to debug channel: {:?}", e);
                        }
                    }
                });

                info!("Debug channel worker initialized");

                tx
            }) {
                Some(tx) => Some(tx),
                None => {
                    warn!("`debug_chat` not present, debug messages will be printed to log");
                    None
                }
            }
        })
        .as_ref()
}

/// Send a debug message to the debug channel if `debug_chat` is set or or log it otherwise
///
/// # Panics
///
/// When debug channel is not initialized
pub fn send_debug(content: &impl ToString) {
    match DEBUG_CHANNEL.get() {
        Some(Some(tx)) => tx
            .send(content.to_string())
            .expect("Background debug channel closed"),
        Some(None) => {
            info!("{}", content.to_string());
        }
        None => {
            panic!("Background debug channel worker is not initialized");
        }
    }
}