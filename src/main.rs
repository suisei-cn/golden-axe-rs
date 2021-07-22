use std::env;

use anyhow::Result;
use log::info;
use teloxide::prelude::{Requester, RequesterExt};
use teloxide::Bot;

use crate::command::handle_command;

mod command;

#[tokio::main]
async fn main() -> Result<()> {
    run().await?;
    Ok(())
}

async fn run() -> Result<()> {
    teloxide::enable_logging_with_filter!(log::LevelFilter::Debug);
    let bot = Bot::from_env().auto_send();
    let bot_name = bot.get_me().await?.user.full_name();
    info!("Running bot as {}", bot_name);

    teloxide::commands_repl(bot, bot_name, handle_command).await;

    Ok(())
}
