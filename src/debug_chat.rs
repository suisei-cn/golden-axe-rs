use std::sync::OnceLock;

use tap::TapOptional;
use teloxide::{
    prelude::{Request, Requester},
    types::ChatId,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tracing::{info, warn};

use crate::{Config, BOT};

static DEBUG_CHANNEL: OnceLock<Option<UnboundedSender<String>>> = OnceLock::new();

/// # Panics
/// When config cannot be parsed
pub fn init() {
    DEBUG_CHANNEL.get_or_init(|| {
        Config::get()
            .debug_chat
            .map(|id| {
                let (tx, mut rx) = unbounded_channel();

                tokio::spawn(async move {
                    let bot = BOT.get().unwrap();
                    while let Some(msg) = rx.recv().await {
                        if let Err(e) = bot.send_message(ChatId(id), msg).send().await {
                            warn!("Failed to send to debug channel: {:?}", e);
                        }
                    }
                });

                info!("Debug channel worker initialized");

                tx
            })
            .tap_none(|| warn!("`debug_chat` not present, debug messages will be printed to log"))
    });
}

/// Send a debug message to the debug channel if `debug_chat` is set or or log
/// it otherwise
///
/// # Panics
///
/// When debug channel is not initialized
pub fn send_debug(content: &impl ToString) {
    match DEBUG_CHANNEL.get() {
        Some(Some(tx)) => {
            let string = content.to_string();
            warn!("{string}");
            tx.send(string).expect("Background debug channel closed");
        }
        Some(None) => {
            info!("{}", content.to_string());
        }
        None => {
            panic!("Debug channel not running");
        }
    }
}

macro_rules! catch {
    ($expr:expr) => {
        if let Err(e) = $expr {
            send_debug(&e);
        }
    };

    ($info:literal, $expr:expr) => {
        if let Err(e) = $expr {
            send_debug(format!("{}: {}", $info, e.to_string()));
        }
    };
}

pub(crate) use catch;
