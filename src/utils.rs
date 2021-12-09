use std::{
    collections::hash_map::DefaultHasher,
    env,
    hash::{Hash, Hasher},
    lazy::SyncOnceCell,
    time::SystemTime,
};

use anyhow::{anyhow, Result};
use log::warn;
use teloxide::{
    prelude::{Request, Requester},
    types::ChatId,
};

pub(crate) static DEBUG_CHANNEL: SyncOnceCell<i64> = SyncOnceCell::new();

pub fn init_debug_channel() {
    if let Err(e) = DEBUG_CHANNEL.get_or_try_init(|| {
        env::var("DEBUG_GROUP_ID")
            .map_err(|_| {
                anyhow!("`DEBUG_GROUP_ID` not set, no error message will be sent to dev group")
            })
            .and_then(|x| {
                x.parse()
                    .map_err(|_| anyhow!("Invalid `DEBUG_GROUP_ID`, should be an `i64`"))
            })
    }) {
        warn!("{}", e)
    }
}
pub async fn send_to_debug_channel(bot: impl Requester, text: impl ToString) -> Result<()> {
    if let Some(channel) = DEBUG_CHANNEL.get() {
        bot.send_message(ChatId::Id(*channel), text.to_string())
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send to debug channel: {:?}", e))?;
    }
    Ok(())
}

pub fn get_run_hash() -> String {
    let mut hasher = DefaultHasher::new();
    env::var("TELOXIDE_TOKEN")
        .expect("Env variable `TELOXIDE_TOKEN` should be set")
        .hash(&mut hasher);
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Wrong system time config")
        .hash(&mut hasher);
    format!("{:X}", hasher.finish())
}
