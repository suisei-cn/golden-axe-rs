#![feature(let_chains)]
#![feature(once_cell)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

mod_use![command, debug_chat, ctx, webhook, config];

use std::{lazy::SyncOnceCell, time::Duration};

use color_eyre::{eyre::ContextCompat, Result};
use mod_use::mod_use;
use teloxide::{adaptors::DefaultParseMode, prelude::*, types::ParseMode};
use tokio::{select, time::sleep};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{
    filter::Targets, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt,
};

use crate::command::{handle_command, ConstBotCommand, COMMANDS};

// (user_id, username)
pub static BOT_INFO: SyncOnceCell<(i64, String)> = SyncOnceCell::new();

type BotType = AutoSend<DefaultParseMode<Bot>>;

#[tokio::main]
#[allow(clippy::redundant_pub_crate)]
async fn main() -> Result<()> {
    drop(dotenv::dotenv());

    let conf = Config::get();
    println!("{:?}", conf);

    tracing_subscriber::fmt()
        .with_max_level(conf.log)
        .without_time()
        .compact()
        .finish()
        .with(
            Targets::new()
                .with_target("hyper::proto", LevelFilter::ERROR)
                .with_target("golden_axe", conf.log)
                .with_default(conf.log),
        )
        .init();

    info!("Start running");

    let bot = Bot::new(&conf.token)
        .parse_mode(ParseMode::Html)
        .auto_send();

    let _ = init(bot.clone());

    select! {
        _ = run(bot) => {},
        _ = tokio::signal::ctrl_c() => {}
    }

    info!("Bot stopped, wrapping up");

    send_debug(&format!("Golden Axe <b>Offline</b> (#{})", conf.run_hash()));

    sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[allow(clippy::future_not_send)]
async fn run(bot: BotType) -> Result<()> {
    let me = bot.get_me().await?.user;

    info!(?me, "Bot logged in");

    let username = me
        .username
        .as_deref()
        .wrap_err_with(|| "Username of bot not set")?;

    BOT_INFO.set((me.id, username.to_owned())).unwrap();

    bot.set_my_commands(COMMANDS.iter().map(ConstBotCommand::into_teloxide))
        .await?;

    send_debug(&format!(
        "Golden Axe <b>Online</b> (#{})",
        Config::get().run_hash()
    ));

    match Config::get().mode {
        BotMode::Webhook { ref domain } => {
            info!("Webhook mode");
            let listener = webhook::setup(&bot, domain).await?;
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
