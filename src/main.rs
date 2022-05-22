#![feature(let_chains)]
#![feature(once_cell)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

mod_use![command, utils, ctx, webhook, config];

use std::lazy::SyncOnceCell;

use anyhow::Result;
use mod_use::mod_use;
use teloxide::{adaptors::DefaultParseMode, prelude::*, types::ParseMode};
use tokio::select;
use tracing::info;

use crate::command::{handle_command, ConstBotCommand, COMMANDS};

// (user_id, username)
pub static BOT_INFO: SyncOnceCell<(i64, String)> = SyncOnceCell::new();

type BotType = AutoSend<DefaultParseMode<Bot>>;

#[tokio::main]
#[allow(clippy::redundant_pub_crate)]
async fn main() -> Result<()> {
    drop(dotenv::dotenv());
    tracing_subscriber::fmt()
        .with_max_level(Config::get().log)
        .init();
    info!("Start running");

    let bot = Bot::from_env().parse_mode(ParseMode::Html).auto_send();

    select! {
        _ = run(bot) => {},
        _ = tokio::signal::ctrl_c() => {}
    }

    debug(&format!(
        "Golden Axe <b>Offline</b> (#{})",
        Config::get().run_hash()
    ));

    Ok(())
}

#[allow(clippy::future_not_send)]
async fn run(bot: BotType) -> Result<()> {
    let _ = init_debug_channel(bot.clone());

    let me = bot.get_me().await?.user;

    info!(?me, "Bot logged in");

    let username = me
        .username
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Username of bot not set"))?;

    BOT_INFO.set((me.id, username.to_owned())).unwrap();

    bot.set_my_commands(COMMANDS.iter().map(ConstBotCommand::into_teloxide))
        .await?;

    debug(&format!(
        "Golden Axe <b>Online</b> (#{})",
        Config::get().run_hash()
    ));

    match Config::get().mode {
        BotMode::Webhook => {
            info!("Webhook mode");
            let listener = webhook::setup(&bot).await?;
            teloxide::commands_repl_with_listener(
                bot,
                username.to_owned(),
                handle_command,
                listener,
            )
            .await;
        }
        BotMode::Poll => {
            info!("Poll mode");
            teloxide::commands_repl(bot, username.to_owned(), handle_command).await;
        }
    }

    Ok(())
}
