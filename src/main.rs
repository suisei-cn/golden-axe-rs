#![feature(let_chains)]
#![feature(once_cell)]

mod_use![command, utils, in_chat_ctx, serve];

use std::{env, lazy::SyncOnceCell};

use anyhow::Result;
use mod_use::mod_use;
use teloxide::{adaptors::DefaultParseMode, prelude::*, types::ParseMode};
use tokio::select;
use tracing::{info, warn};

use crate::command::{handle_command, ConstBotCommand, COMMANDS};

pub static RUN_HASH: SyncOnceCell<String> = SyncOnceCell::new();
type BotType = AutoSend<DefaultParseMode<Bot>>;

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = dotenv::dotenv() {
        warn!("Dotenv failed: {}", e)
    }
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "INFO")
    };
    tracing_subscriber::fmt().init();
    info!("Start running");

    let bot = Bot::from_env().parse_mode(ParseMode::Html).auto_send();

    select! {
        _ = run(&bot) => {},
        _ = tokio::signal::ctrl_c() => {}
    }

    send_to_debug_channel(
        &bot,
        format!("Golden Axe <b>Offline</b> (#{})", RUN_HASH.get().unwrap()),
    )
    .await?;

    Ok(())
}

async fn run(bot: &BotType) -> Result<()> {
    init_debug_channel();

    let _ = RUN_HASH.set(gen_run_hash());

    let user = bot.get_me().await?.user;
    info!(?user, "Bot logged in");
    let username = user
        .username
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Username of bot not set"))?;
    bot.set_my_commands(COMMANDS.iter().map(ConstBotCommand::into_teloxide))
        .await?;

    send_to_debug_channel(
        &bot,
        format!("Golden Axe <b>Online</b> (#{})", RUN_HASH.get().unwrap()),
    )
    .await?;

    match env::var("ENV").map(|x| x.to_lowercase()) {
        Ok(content) if content == "production" => {
            info!("Webhook mode");
            let listener = setup_webhook(&bot).await?;
            teloxide::commands_repl_with_listener(
                bot.clone(),
                username.to_owned(),
                handle_command,
                listener,
            )
            .await
        }
        _ => {
            info!("Poll mode");
            teloxide::commands_repl(bot.clone(), username.to_owned(), handle_command).await
        }
    }

    Ok(())
}
