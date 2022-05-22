use std::time::Duration;

use anyhow::{anyhow, Result};
use teloxide::{
    prelude::*,
    types::{Administrator, ChatMember, ChatMemberKind, User},
};
use tokio::{time::sleep, try_join};
use tracing::warn;

use crate::{BotType, Ctx, BOT_INFO};

pub struct InChatCtx<'a> {
    pub bot: &'a BotType,
    pub sender: &'a User,
    pub chat_id: i64,
    // The bot in Group (Received the message)
    pub rx: ChatMember,
    // Sender in group (Sent the message)
    pub tx: ChatMember,
}

impl<'a> InChatCtx<'a> {
    /// Initialize in chat context from [`UpdateWithCtx`]
    /// # Errors
    /// Failed when no sender
    ///
    /// # Panics
    /// When bot info is not initialized
    ///
    /// [`UpdateWithCtx`]: teloxide::prelude::UpdateWithCtx
    pub async fn from_ctx(ctx: &'a Ctx) -> Result<InChatCtx<'a>> {
        let sender = ctx.update.from().ok_or_else(|| anyhow!("No sender"))?;
        let chat_id = ctx.chat_id();
        let bot = &ctx.requester;

        let (rx, tx) = try_join!(
            bot.get_chat_member(chat_id, BOT_INFO.get().expect("Bot info not initialized").0),
            bot.get_chat_member(chat_id, sender.id)
        )?;

        Ok(Self {
            bot,
            sender,
            chat_id,
            rx,
            tx,
        })
    }

    /// # Errors
    /// Failed when failed to promote member. This method does not assure that the bot is privileged enough to
    /// promote the member, so it should be checked by the caller.
    pub async fn promote(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id, self.tx.user.id)
            .can_invite_users(true)
            .send()
            .await
            .map_err(|e| anyhow!("Promote member error: {}", e))?;
        Ok(())
    }

    /// # Errors
    /// Failed when failed to demote the member. This method does not assure that the bot is privileged enough to
    /// promote the member, so it should be checked by the caller.
    pub async fn demote(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id, self.tx.user.id)
            .send()
            .await
            .map_err(|e| anyhow!("Demote member error: {}", e))?;
        Ok(())
    }

    async fn set_title(&self, title: impl Into<String> + Send) -> Result<(), &str> {
        self.bot
            .set_chat_administrator_custom_title(self.chat_id, self.sender.id, title)
            .await
            .map_err(|error| {
                warn!(%error);
                "Failed to set title"
            })?;
        Ok(())
    }

    /// Change title to `title` for the sender.
    ///
    /// # Errors
    ///
    /// Failed when not privileged enough, or failed to set title.
    pub async fn change_title(&self, title: impl Into<String> + Send) -> Result<(), &str> {
        #[allow(clippy::enum_glob_use)]
        use ChatMemberKind::*;
        match self.tx.kind {
            Administrator(_) => {
                self.can_edit()?;
                self.set_title(title).await
            }
            Member => {
                self.can_promote()?;
                self.promote().await.map_err(|error| {
                    warn!(%error);
                    "Failed to promote"
                })?;
                sleep(Duration::from_secs_f32(0.5)).await;
                self.set_title(title).await
            }
            Owner(_) => Err("Cannot modify owner"),
            _ => Err("I can't edit you because of your status (Restricted, Left or Banned)"),
        }
    }

    /// Ensure that the sender is privileged enough to edit the user.
    ///
    /// This means:
    /// - The bot is the owner of the chat (ultimate privilege)
    /// - The sender an admin promoted by this bot
    /// - The sender is an user that is going to be promoted
    ///
    /// # Errors
    /// Failed when not privileged enough.
    pub const fn can_edit(&self) -> Result<(), &str> {
        match self.rx.kind {
            ChatMemberKind::Owner(_) => Ok(()),
            ChatMemberKind::Administrator(_) => match self.tx.kind {
                ChatMemberKind::Administrator(Administrator { can_be_edited, .. }) => {
                    if can_be_edited {
                        Ok(())
                    } else {
                        Err("I can't change your info (are you promoted by others?)")
                    }
                }
                ChatMemberKind::Member => Ok(()),
                _ => Err("I can't edit you because of your status"),
            },
            _ => Err("I'm not an admin, please promote me with promote privilege first"),
        }
    }

    /// Ensure that the sender is privileged enough to promote the user.
    /// This means that the user [can be edited](#method.can_edit) and the bot has the `can_promote_members` privilege
    ///
    /// # Errors
    /// Failed when not privileged enough.
    pub fn can_promote(&self) -> Result<(), &str> {
        self.can_edit().and_then(|_| {
            if self.rx.kind.can_promote_members() {
                Ok(())
            } else {
                Err("I don't have the privilege to promote others, please contant admin")
            }
        })
    }
}
