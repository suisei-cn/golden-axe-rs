use anyhow::{anyhow, Result};
use teloxide::{
    prelude::*,
    types::{Administrator, ChatMember, ChatMemberKind, User},
};

use crate::{BotType, Ctx};

pub struct InChatCtx<'a> {
    pub bot: &'a BotType,
    pub sender: &'a User,
    pub chat_id: i64,
    // Me in Group (Received the message)
    pub rx: ChatMember,
    // Sender in group (Sent the message)
    pub tx: ChatMember,
}

impl<'a> InChatCtx<'a> {
    pub async fn from_ctx(ctx: &'a Ctx) -> Result<InChatCtx<'a>> {
        let sender = ctx.update.from().ok_or_else(|| anyhow!("No sender"))?;
        let chat_id = ctx.chat_id();
        let bot = &ctx.requester;
        let rx = bot
            .get_chat_member(chat_id, bot.get_me().await?.user.id)
            .await?;
        let tx = bot.get_chat_member(chat_id, sender.id).await?;
        Ok(Self {
            bot,
            sender,
            chat_id,
            rx,
            tx,
        })
    }

    pub async fn promote(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id, self.tx.user.id)
            .can_invite_users(true)
            .send()
            .await
            .map_err(|e| anyhow!("Promote member error: {}", e))?;
        Ok(())
    }

    pub async fn demote(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id, self.tx.user.id)
            .send()
            .await
            .map_err(|e| anyhow!("Demote member error: {}", e))?;
        Ok(())
    }

    async fn set_title(&self, title: impl Into<String>) -> Result<()> {
        self.bot
            .set_chat_administrator_custom_title(self.chat_id, self.sender.id, title)
            .await?;
        Ok(())
    }

    pub async fn change_title(&self, title: impl Into<String>) -> Result<(), &str> {
        match self.tx.kind {
            ChatMemberKind::Administrator(_) => {
                self.can_edit()?;
                self.set_title(title)
                    .await
                    .map_err(|_| "Failed to set title")
            }
            ChatMemberKind::Member => {
                self.can_promote()?;
                self.promote().await.map_err(|_| "Failed to promote")?;
                self.set_title(title)
                    .await
                    .map_err(|_| "Failed to set title")
            }
            _ => Err("I can't edit you because of your status"),
        }
    }

    pub fn can_edit(&self) -> Result<(), &str> {
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
            _ => Err("I'm not an admin, please contact admin"),
        }
    }

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
