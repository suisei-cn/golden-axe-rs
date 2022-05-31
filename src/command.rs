use std::{future::Future, lazy::SyncLazy};

use color_eyre::Result;
use teloxide::{
    types::{Message, User},
    utils::command::BotCommands,
};
use tracing::{info, warn};

use crate::{send_debug, BotType, Ctx, Loaded};

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
}

#[test]
fn test_command() {
    println!("{}", Command::descriptions());
    println!("{:#?}", Command::bot_commands());
}

async fn handle<'a, Func, Fut>(ctx: Ctx<'a, ()>, func: Func) -> Result<()>
where
    Fut: Future<Output = Result<()>> + Send,
    Func: FnOnce(Ctx<'a, Loaded>) -> Fut + Send,
{
    let light = ctx.clone();
    let loaded = ctx.fetch().await?;

    let inner = move || async {
        loaded.assert_in_group()?;
        func(loaded).await?;
        Result::<()>::Ok(())
    };

    match inner().await {
        Ok(()) => {
            light
                .reply_to("Done! Wait for a while to take effect.")
                .await
        }
        Err(e) => {
            warn!("{}", e);
            light.reply_to("Internal Error").await?;
            send_debug(&e);
            Ok(())
        }
    }
}

pub async fn handle_command(bot: BotType, msg: Message, command: Command) -> Result<()> {
    let from = msg.from().map(User::full_name);
    let ctx = Ctx::new(&bot, &msg)?;

    info!(?from, ?command, "Handing");

    match command {
        Command::Help => {
            static DESC: SyncLazy<String> = SyncLazy::new(|| Command::descriptions().to_string());
            ctx.reply_to(&*DESC).await
        }

        Command::Title { title } => {
            handle(ctx, |ctx| async move {
                ctx.prep_promote().await?;
                ctx.set_title(title).await
            })
            .await
        }

        Command::Demote => {
            handle(ctx, |ctx| async move {
                ctx.prep_promote().await?;
                ctx.demote().await
            })
            .await
        }

        Command::Anonymous => {
            handle(ctx, |ctx| async move {
                ctx.prep_promote().await?;
                ctx.set_anonymous().await
            })
            .await
        }
    }
}
