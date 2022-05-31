#![allow(clippy::enum_glob_use)]
#![allow(clippy::future_not_send)]

use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use color_eyre::{
    eyre::{bail, ensure, eyre, Context},
    Result,
};
use tap::TapFallible;
use teloxide::{
    payloads::{PromoteChatMemberSetters, SendMessageSetters},
    prelude::*,
    types::{
        Administrator as Admin, ChatId, ChatKind, ChatMember, ChatMemberKind, ChatPublic,
        PublicChatKind, User, UserId,
    },
};
use tokio::{time::sleep, try_join};
use tracing::warn;

use crate::{BotType, BOT_INFO};

/// Context of a "conversion", which is formed when an user sends a command to
/// the bot.
///
/// The context has two state: `Light` (just `()`, nothing) and [`Loaded`]. The
/// former is used when [`ChatMember`] information (of both sender and the bot)
/// is not needed, like to assert the user is from a group or reply to message.
/// And the [`Loaded`] state is used when the [`ChatMember`] information is
/// needed, like to change the user's title and more.
///
/// To convert from `Light` state to [`Loaded`] state, use [`fetch`]
/// method. This will consume the original [`Ctx<()>`] and return a
/// [`Ctx<Loaded>`] state.
///
/// Under the hood `Light` is just three ordinary reference to
///
/// [`fetch`]: Ctx::fetch
#[derive(Debug, Clone)]
pub struct Ctx<'a, S> {
    bot: &'a BotType,
    msg: &'a Message,
    conversation: S,
}

/// State of the context representing conversation information has been fetched.
#[derive(Clone)]
pub struct Loaded(Box<(ChatMember, ChatMember)>);

impl Deref for Loaded {
    type Target = (ChatMember, ChatMember);

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Loaded {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> Ctx<'a, ()> {
    /// Create a new light context.
    ///
    /// # Errors
    /// When the message has no sender
    pub fn new(bot: &'a BotType, msg: &'a Message) -> Result<Self> {
        ensure!(msg.from().is_some(), "The message has no sender");

        Ok(Self {
            bot,
            msg,
            conversation: (),
        })
    }
}

impl<'a, S> Ctx<'a, S> {
    /// Initialize in chat context from [`UpdateWithCtx`]
    ///
    /// # Errors
    /// Failed when no sender
    ///
    /// [`UpdateWithCtx`]: teloxide::prelude::UpdateWithCtx
    pub async fn new_full(bot: &'a BotType, msg: &'a Message) -> Result<Ctx<'a, Loaded>> {
        let ctx = Ctx::new(bot, msg)?;
        ctx.fetch().await
    }

    /// Get the chat id of current conversation
    #[inline]
    #[must_use]
    pub const fn chat_id(&self) -> ChatId {
        self.msg.chat.id
    }

    /// Get the sender
    #[inline]
    #[must_use]
    pub fn sender(&self) -> &User {
        self.msg
            .from()
            .expect("Sender should be enforced during initialization")
    }

    /// Get the [`UserId`] of current sender
    #[inline]
    #[must_use]
    pub fn sender_id(&self) -> UserId {
        self.sender().id
    }

    /// Fetches the conversation information from the bot and turn self into
    /// [`Full`].
    ///
    /// # Errors
    /// If the chat member information cannot be fetched.
    pub async fn fetch(self) -> Result<Ctx<'a, Loaded>> {
        let (rx, tx) = try_join!(
            self.bot.get_chat_member(
                self.msg.chat.id,
                BOT_INFO.get().expect("Bot info not initialized").0
            ),
            self.bot.get_chat_member(self.msg.chat.id, self.sender().id)
        )
        .tap_err(|error| {
            warn!(%error);
        })?;
        Ok(Ctx {
            bot: self.bot,
            msg: self.msg,
            conversation: Loaded(Box::new((rx, tx))),
        })
    }

    /// Set title of user
    ///
    /// # Errors
    /// If the user cannot be set a title or requesting error.
    pub async fn set_title(&self, title: impl Into<String> + Send) -> Result<()> {
        self.bot
            .set_chat_administrator_custom_title(self.chat_id(), self.sender_id(), title)
            .await
            .map_err(|error| {
                warn!(%error);
                eyre!("Failed to set title")
            })?;
        Ok(())
    }

    /// Make the user anonymous
    ///
    /// # Errors
    /// If the user cannot be promoted or requesting error.
    pub async fn set_anonymous(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id(), self.sender_id())
            .can_invite_users(true)
            .is_anonymous(true)
            .await
            .map_err(|error| {
                warn!(%error);
                eyre!("Failed to make anonymous")
            })?;
        Ok(())
    }

    /// Run [`promote_chat_member`], with `can_invite_users` privilege.
    ///
    /// # Errors
    /// Failed when failed to promote member. This method does not assure that
    /// the bot is privileged enough to promote the member, so it should be
    /// checked by the caller.
    ///
    /// [`promote_chat_member`]: https://core.telegram.org/bots/api#promotechatmember
    pub async fn promote(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id(), self.sender_id())
            .can_invite_users(true)
            .send()
            .await
            .wrap_err("Promote member error")?;
        Ok(())
    }

    /// Run [`promote_chat_member`], with all privileges being false.
    ///
    /// # Errors
    /// Failed when failed to demote the member. This method does not assure
    /// that the bot is privileged enough to promote the member, so it
    /// should be checked by the caller.
    pub async fn demote(&self) -> Result<()> {
        self.bot
            .promote_chat_member(self.chat_id(), self.sender_id())
            .send()
            .await
            .wrap_err("Demote member error")?;
        Ok(())
    }

    /// Reply to the sender with a message.
    ///
    /// # Errors
    /// When the message sending fails.
    pub async fn reply_to(&self, text: impl Into<String> + Send) -> Result<()> {
        self.bot
            .send_message(self.chat_id(), text)
            .reply_to_message_id(self.msg.id)
            .await?;
        Ok(())
    }

    /// A guard method to assure the user is in a public group
    ///
    /// # Errors
    /// If the user is not in a public group.
    pub fn assert_in_group(&self) -> Result<()> {
        if matches!(
            self.msg.chat.kind,
            ChatKind::Public(ChatPublic {
                kind: PublicChatKind::Group(_) | PublicChatKind::Supergroup(_),
                ..
            })
        ) {
            Ok(())
        } else {
            bail!("This command can only be used in group")
        }
    }
}

impl<'a> Ctx<'a, Loaded> {
    #[inline]
    #[must_use]
    pub const fn sender_in_chat(&self) -> &ChatMember {
        &self.conversation.0.0
    }

    #[inline]
    #[must_use]
    pub const fn me_in_chat(&self) -> &ChatMember {
        &self.conversation.0.1
    }

    /// Prepare for promotion
    ///
    /// This will check for proper privileges according to status of the
    /// conversation.
    ///
    /// # Errors
    ///
    /// If the bot or the user is not privileged enough or suitable to promote
    /// or be promoted.
    pub async fn prep_promote(&self) -> Result<()> {
        use ChatMemberKind::*;

        match self.sender_in_chat().kind {
            Administrator(_) => self.assert_editable()?,
            Member => {
                self.assert_promotable()?;
                self.promote().await.map_err(|error| {
                    warn!(%error);
                    eyre!("Failed to promote")
                })?;
                // Wait a while for the promotion to take effect.
                sleep(Duration::from_secs_f32(0.5)).await;
            }
            ref k => bail!(
                "I can't edit you because of your status({})",
                chat_member_kind_to_str(k)
            ),
        }
        Ok(())
    }

    /// Ensure that the bot is privileged enough to edit the user.
    ///
    /// This means one of these situations:
    /// - The bot is the owner of the chat (ultimate privilege)
    /// - The sender is an admin promoted by this bot
    /// - The sender is an user that is going to be promoted
    ///
    /// # Errors
    /// Failed when not privileged enough.
    pub fn assert_editable(&self) -> Result<()> {
        use ChatMemberKind::*;

        match self.me_in_chat().kind {
            Owner(_) => Ok(()),
            Administrator(_) => match self.sender_in_chat().kind {
                Administrator(Admin { can_be_edited, .. }) => {
                    ensure!(
                        can_be_edited,
                        "I can't change your info (are you promoted by others?)"
                    );
                    Ok(())
                }
                Member => Ok(()),
                ref k => bail!(
                    "I can't edit you because of your status({})",
                    chat_member_kind_to_str(k)
                ),
            },
            _ => bail!("I'm not an admin, please promote me with promotion privilege first"),
        }
    }

    /// Ensure that the sender is privileged enough to promote the user.
    /// This means that the user [can be edited](#method.can_edit) and the bot
    /// has the `can_promote_members` privilege
    ///
    /// # Errors
    /// Failed when not privileged enough.
    pub fn assert_promotable(&self) -> Result<()> {
        self.assert_editable().and_then(|_| {
            ensure!(
                self.me_in_chat().kind.can_promote_members(),
                "I don't have the privilege to promote others, please contant admin"
            );
            Ok(())
        })
    }
}

const fn chat_member_kind_to_str(kind: &ChatMemberKind) -> &'static str {
    match kind {
        ChatMemberKind::Administrator(..) => "admin",
        ChatMemberKind::Member => "member",
        ChatMemberKind::Owner(_) => "owner",
        ChatMemberKind::Restricted(_) => "restricted",
        ChatMemberKind::Left => "left",
        ChatMemberKind::Banned(_) => "banned",
    }
}
