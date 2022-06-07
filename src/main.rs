#![feature(let_chains)]
#![feature(once_cell)]
#![feature(iterator_try_collect)]
#![feature(box_into_inner)]
#![allow(clippy::module_name_repetitions)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::all)]

mod_use![bot, debug_chat, ctx, config, server];

use std::{lazy::SyncOnceCell, time::Duration};

use color_eyre::Result;
use mod_use::mod_use;
use teloxide::{
    adaptors::DefaultParseMode,
    prelude::*,
    types::{ParseMode, UserId},
};
use tokio::{select, time::sleep};
use tracing::{info, level_filters::LevelFilter, warn};
use tracing_subscriber::{
    filter::Targets, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt,
};

// (user_id, username)
pub static BOT_INFO: SyncOnceCell<(UserId, String)> = SyncOnceCell::new();
pub static BOT: SyncOnceCell<BotType> = SyncOnceCell::new();

type BotType = AutoSend<DefaultParseMode<Bot>>;

#[tokio::main]
#[allow(clippy::redundant_pub_crate)]
async fn main() -> Result<()> {
    drop(dotenv::dotenv());

    let conf = Config::get();

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

    let bot: BotType = Bot::new(&conf.token)
        .parse_mode(ParseMode::Html)
        .auto_send();
    BOT.set(bot.clone()).unwrap();

    let db = sled::open(&conf.db_path).unwrap();

    let _ = debug_chat::init();

    select! {
        _ = server::run() => {},
        _ = bot::run(bot, db) => {},
        _ = tokio::signal::ctrl_c() => {}
    }

    info!("Bot stopped, wrapping up");

    send_debug(&format!("Golden Axe <b>Offline</b> (#{})", conf.run_hash()));

    sleep(Duration::from_secs(1)).await;

    Ok(())
}
