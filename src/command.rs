use color_eyre::Result;
use teloxide::{
    prelude::*,
    types::{ChatKind, ChatPublic, Message, PublicChatKind, User},
    utils::command::BotCommand,
};
use tracing::info;

use crate::{BotType, InChatCtx};

pub type Ctx = UpdateWithCx<BotType, Message>;

#[derive(Clone, Copy, Debug)]
pub struct ConstBotCommand<'a> {
    pub command: &'a str,
    pub description: &'a str,
}

impl<'a> ConstBotCommand<'a> {
    pub fn into_teloxide(val: &ConstBotCommand<'a>) -> teloxide::types::BotCommand {
        teloxide::types::BotCommand {
            command: val.command.into(),
            description: val.description.into(),
        }
    }
}

pub const COMMANDS: &[ConstBotCommand] = &[
    ConstBotCommand {
        command: "help",
        description: "display help text",
    },
    ConstBotCommand {
        command: "title",
        description: "change my title",
    },
    ConstBotCommand {
        command: "demote",
        description: "remove my admin and title",
    },
];

#[derive(BotCommand, Debug, Clone)]
#[command(rename = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "display this text.")]
    Help,
    #[command(description = "change my title.")]
    Title { title: String },
    #[command(description = "demote me and remove my title")]
    Demote,
}

macro_rules! command {
    ($cx:ident, $handler:expr) => {
        if let Err(e) = $handler {
            ::tracing::warn!("{}", e);
            $cx.reply_to("Internal Error").await?;
            $crate::send_debug(&e);
        }
    };
}

macro_rules! assert_in_group {
    ($cx:ident) => {
        if !matches!(
            $cx.update.chat.kind,
            ChatKind::Public(ChatPublic {
                kind: PublicChatKind::Group(_) | PublicChatKind::Supergroup(_),
                ..
            })
        ) {
            $cx.reply_to("Call this command in a group or supergroup")
                .await?;
            return Ok(());
        }
    };
}

pub async fn handle_command(cx: Ctx, command: Command) -> Result<()> {
    let from = cx.update.from().map(User::full_name);

    info!(?from, ?command);

    match command {
        Command::Help => {
            cx.answer(Command::descriptions()).await?;
        }
        Command::Title { title } => {
            command!(cx, set_title(&cx, title).await);
        }
        Command::Demote => {
            command!(cx, demote_me(&cx).await);
        }
    };

    Ok(())
}

async fn demote_me(cx: &Ctx) -> Result<()> {
    assert_in_group!(cx);

    let temp_ctx = InChatCtx::from_ctx(cx).await?;

    if let Err(e) = temp_ctx.can_promote() {
        cx.reply_to(e).await?;
    } else {
        temp_ctx.demote().await?;
        cx.reply_to("Done! Wait for a while to take effect.")
            .await?;
    }

    Ok(())
}

async fn set_title(cx: &Ctx, title: String) -> Result<()> {
    assert_in_group!(cx);

    let temp_ctx = InChatCtx::from_ctx(cx).await?;

    if let Err(e) = temp_ctx.change_title(title).await {
        cx.reply_to(e).await?;
    } else {
        cx.reply_to("Done! Wait for a while to take effect.")
            .await?;
    }

    Ok(())
}
