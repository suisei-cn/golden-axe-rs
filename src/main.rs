#![feature(once_cell)]

mod_use![command, utils, in_chat_ctx, serve];

use std::{collections::HashMap, env, lazy::SyncOnceCell};

use anyhow::Result;
use log::{info, warn};
use mod_use::mod_use;
use teloxide::{adaptors::DefaultParseMode, prelude::*, types::ParseMode};

use crate::command::{handle_command, ConstBotCommand, COMMANDS};

pub static RUN_HASH: SyncOnceCell<String> = SyncOnceCell::new();
type BotType = AutoSend<DefaultParseMode<Bot>>;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Start running");
    if let Err(e) = dotenv::dotenv() {
        warn!("Dotenv failed: {}", e)
    }
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "INFO")
    };
    pretty_env_logger::init();
    info!("{:#?}", env::vars().collect::<HashMap<_, _>>());
    run().await?;
    Ok(())
}

async fn run() -> Result<()> {
    init_debug_channel();

    let _ = RUN_HASH.set(get_run_hash());

    let bot = Bot::from_env().parse_mode(ParseMode::Html).auto_send();
    let bot_user = bot.get_me().await?.user;
    info!("Running as: {:#?}", bot_user);
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
            let webhook = webhook(&bot).await?;
            teloxide::commands_repl_with_listener(
                bot.clone(),
                bot_user.full_name(),
                handle_command,
                webhook,
            )
            .await
        }
        _ => {
            info!("Poll mode");
            teloxide::commands_repl(bot.clone(), bot_user.full_name(), handle_command).await
        }
    }

    send_to_debug_channel(
        &bot,
        format!("Golden Axe <b>Offline</b> (#{})", RUN_HASH.get().unwrap()),
    )
    .await?;

    Ok(())
}
