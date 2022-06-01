use std::{
    convert::Infallible,
    future::{ready, Future},
    lazy::SyncLazy,
    sync::Arc,
};

use color_eyre::{
    eyre::{bail, ensure, eyre, ContextCompat},
    Result,
};
use sled::Db;
use teloxide::{
    dispatching::update_listeners, prelude::*, types::User, utils::command::BotCommands,
};
use tracing::info;

use crate::{catch, send_debug, BotType, Config, Ctx, BOT_INFO};

#[derive(BotCommands, Debug, Clone)]
#[command(rename = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "Display this text.")]
    Help,
    #[command(description = "Display this text.")]
    Start,
    #[command(description = "Change my title.")]
    Title { title: String },
    #[command(description = "Remove specific title")]
    RemoveTitle { title: String },
    #[command(description = "Get all titles being used")]
    Titles,
    #[command(description = "Demote me and remove my title")]
    Demote { username: String },
    #[command(description = "Demote everyone and remove all titles in chat")]
    Nuke,
    #[command(description = "Make me anonymous")]
    Anonymous,
    #[command(description = "Make me un-anonymous")]
    DeAnonymous,
}

#[test]
fn test_command() {
    println!("{}", Command::descriptions());
    println!("{:#?}", Command::bot_commands());
}

#[allow(clippy::future_not_send)]
pub async fn run(bot: BotType, db: sled::Db) -> Result<()> {
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

async fn handle_command(
    bot: BotType,
    msg: Message,
    command: Command,
    db: Db,
) -> Result<(), Infallible> {
    let from = msg.from().map(User::full_name);
    let ctx = Ctx::new(&bot, &msg, &db).expect("Command messages should have sender");

    info!(?from, ?command, "Handing");

    catch!(match command {
        Command::Help | Command::Start => {
            static DESC: SyncLazy<String> = SyncLazy::new(|| Command::descriptions().to_string());
            ctx.reply_to(&*DESC).await
        }
        cmd => {
            ctx.handle_with(|mut ctx| async move {
                match cmd {
                    Command::Title { title } => {
                        ensure!(!title.is_empty(), "Title cannot be empty");
                        ctx.prep_edit().await?;
                        ctx.set_title(title).await?;
                        ctx.done().await
                    }
                    Command::RemoveTitle { title } => {
                        ctx.assert_sender_owner()?;
                        ctx.remove_title_with_sig(&title)?;
                        ctx.done().await
                    }
                    Command::Demote { username } => match username.as_str() {
                        "" => {
                            ctx.assert_editable()?;
                            ctx.assert_bot_promotable()?;
                            ctx.demote().await?;
                            ctx.remove_title_with_id()?;
                            ctx.done().await
                        }
                        string if string.starts_with('@') && string.len() > 1 => {
                            ctx.assert_sender_owner()?;
                            let name = &string[1..];
                            info!("{name:?}");
                            let target = ctx
                                .find_admin_with_username(name)
                                .await?
                                .ok_or_else(|| eyre!("No such user"))?;

                            ctx.with_sender(target, |ctx| async move {
                                ctx.assert_editable()?;
                                ctx.assert_bot_promotable()?;
                                ctx.demote().await?;
                                ctx.remove_title_with_id()?;
                                ctx.done().await
                            })
                            .await
                        }
                        _ => {
                            bail!(
                                "format: /demote to demote yourself or /demote @someone if you're \
                                 owner"
                            )
                        }
                    },
                    Command::Anonymous => {
                        ctx.assert_bot_anonymous()?;
                        if ctx.is_anonymous() {
                            bail!("You are already anonymous")
                        }
                        if ctx.get_record_with_id()?.is_none() {
                            bail!("Before making anonymous, use /title first to register")
                        }
                        ctx.prep_edit().await?;
                        ctx.set_anonymous().await?;
                        ctx.done().await
                    }
                    Command::DeAnonymous => {
                        ctx.de_anonymous().await?;
                        ctx.done().await
                    }
                    Command::Nuke => {
                        ctx.assert_sender_owner()?;
                        ctx.nuke().await?;
                        ctx.done().await
                    }
                    Command::Titles => {
                        let keys = ctx.list_titles()?;
                        let show = if keys.is_empty() {
                            "No titles found.".to_owned()
                        } else {
                            let titles = keys
                                .iter()
                                .map(std::string::ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("\n");
                            format!("<code>in Chat({}):</code>\n{}", keys[0].chat_id, titles)
                        };
                        ctx.reply_to(&show).await
                    }
                    Command::Help | Command::Start => unreachable!(),
                }
            })
            .await
        }
    });
    catch!(db.flush_async().await);
    Ok(())
}
