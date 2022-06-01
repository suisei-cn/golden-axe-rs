use std::{convert::Infallible, lazy::SyncLazy};

use color_eyre::{eyre::ensure, Result};
use sled::Db;
use teloxide::{
    types::{Message, User},
    utils::command::BotCommands,
};
use tracing::info;

use crate::{catch, send_debug, BotType, Ctx};

#[derive(BotCommands, Debug, Clone)]
#[command(rename = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "Display this text.")]
    Help,
    #[command(description = "Change my title.")]
    Title { title: String },
    #[command(description = "Demote me and remove my title")]
    Demote,
    #[command(description = "Make me anonymous")]
    Anonymous,
    #[command(description = "Make me un-anonymous")]
    DeAnonymous,
    #[command(description = "Get all titles being used")]
    Titles,
}

#[test]
fn test_command() {
    println!("{}", Command::descriptions());
    println!("{:#?}", Command::bot_commands());
}

pub async fn handle_command(
    bot: BotType,
    msg: Message,
    command: Command,
    db: Db,
) -> Result<(), Infallible> {
    let from = msg.from().map(User::full_name);
    let ctx = Ctx::new(&bot, &msg, &db).expect("Command messages should have sender");

    info!(?from, ?command, "Handing");

    catch!(match command {
        Command::Help => {
            static DESC: SyncLazy<String> = SyncLazy::new(|| Command::descriptions().to_string());
            ctx.reply_to(&*DESC).await
        }
        cmd => {
            ctx.handle_with(|ctx| async move {
                match cmd {
                    Command::Title { title } => {
                        ctx.prep_edit().await?;
                        ctx.set_title(title).await?;
                        ctx.done().await
                    }
                    Command::Demote => {
                        ctx.assert_editable()?;
                        ctx.assert_promotable()?;
                        ctx.demote().await?;
                        ctx.remove_title_with_id()?;
                        ctx.done().await
                    }
                    Command::Anonymous => {
                        ctx.assert_anonymous()?;
                        ensure!(
                            ctx.get_record_with_id()?.is_some(),
                            "Before making anonymous, use /title first to register"
                        );
                        ctx.prep_edit().await?;
                        ctx.set_anonymous().await?;
                        ctx.done().await
                    }
                    Command::DeAnonymous => {
                        ctx.de_anonymous().await?;
                        ctx.done().await
                    }
                    Command::Titles => {
                        ctx.assert_sender_admin()?;
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
                    Command::Help => unreachable!(),
                }
            })
            .await
        }
    });
    catch!(db.flush_async().await);
    Ok(())
}
