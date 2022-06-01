#![feature(let_chains)]
#![feature(once_cell)]
#![feature(iterator_try_collect)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

mod_use![command, debug_chat, ctx, config, server];

use std::{
    future::{ready, Future},
    lazy::SyncOnceCell,
    sync::Arc,
    time::Duration,
};

use color_eyre::{eyre::ContextCompat, Result};
use mod_use::mod_use;
use teloxide::{
    adaptors::DefaultParseMode,
    dispatching::update_listeners,
    prelude::*,
    types::{ParseMode, UserId},
    utils::command::BotCommands,
};
use tokio::{select, time::sleep};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{
    filter::Targets, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt,
};

use crate::command::handle_command;

// (user_id, username)
pub static BOT_INFO: SyncOnceCell<(UserId, String)> = SyncOnceCell::new();

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

    let db = sled::open(&conf.db_path).unwrap();

    let _ = debug_chat::init(bot.clone());

    select! {
        _ = run(bot, db) => {},
        _ = tokio::signal::ctrl_c() => {}
    }

    info!("Bot stopped, wrapping up");

    send_debug(&format!("Golden Axe <b>Offline</b> (#{})", conf.run_hash()));

    sleep(Duration::from_secs(1)).await;

    Ok(())
}

#[allow(clippy::future_not_send)]
async fn run(bot: BotType, db: sled::Db) -> Result<()> {
    let me = bot.get_me().await?.user;

    info!(?me, "Bot logged in");

    let username = me
        .username
        .as_deref()
        .wrap_err_with(|| "Username of bot not set")?;

    BOT_INFO.set((me.id, username.to_owned())).unwrap();

    bot.set_my_commands(Command::bot_commands()).await?;

    send_debug(&format!(
        "Golden Axe <b>Online</b>, running as @{username} (#{})",
        Config::get().run_hash()
    ));

    info!("Poll mode");

    let mut deps = DependencyMap::new();
    deps.insert(db);

    Dispatcher::builder(
        bot.clone(),
        Update::filter_message()
            .filter_command::<Command>()
            .chain(dptree::endpoint(handle_command)),
    )
    .default_handler(ignore_update)
    .dependencies(deps)
    .build()
    .setup_ctrlc_handler()
    .dispatch_with_listener(
        update_listeners::polling_default(bot).await,
        LoggingErrorHandler::new(),
    )
    .await;

    Ok(())
}

fn ignore_update(_: Arc<Update>) -> impl Future<Output = ()> {
    ready(())
}
