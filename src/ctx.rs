#![allow(clippy::enum_glob_use)]
#![allow(clippy::future_not_send)]

use std::{
    fmt::{self, Display},
    future::Future,
    time::Duration,
};

use color_eyre::{
    eyre::{bail, ensure, eyre, Context, ContextCompat},
    Result,
};
use futures::future::try_join_all;
use sled::{Db, IVec};
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
use tracing::info;

use crate::{catch, send_debug, BotType, Config, BOT, BOT_INFO};

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
    db: &'a Db,
    sender: User,
    is_anonymous: bool,
    conversation: S,
}

/// State of the context representing conversation information has been fetched.
#[derive(Clone)]
pub struct Loaded {
    pub me: Box<ChatMember>,
    pub sender: Box<ChatMember>,
}

impl Loaded {
    #[must_use]
    pub fn new(me: ChatMember, sender: ChatMember) -> Self {
        Self {
            me: Box::new(me),
            sender: Box::new(sender),
        }
    }

    pub fn swap_bot(&mut self, bot: ChatMember) -> Box<ChatMember> {
        std::mem::replace(&mut self.me, Box::new(bot))
    }

    pub fn swap_sender(&mut self, sender: ChatMember) -> Box<ChatMember> {
        std::mem::replace(&mut self.sender, Box::new(sender))
    }
}

impl<'a, 'u> Ctx<'a, ()> {
    /// Create a new light context.
    ///
    /// # Errors
    /// When the message has no sender
    pub fn new(bot: &'a BotType, msg: &'a Message, db: &'a Db) -> Result<Self> {
        let sender = match msg.from().cloned() {
            Some(sender) => sender,
            None => bail!("Message has no sender"),
        };

        Ok(Self {
            bot,
            msg,
            db,
            sender,
            is_anonymous: false,
            conversation: (),
        })
    }

    /// Handle the command with the given function.
    /// This method wraps the function and send all errors directly to the
    /// sender.
    ///
    /// # Errors
    /// Only fetching error and network error will be emitted. Logic errors are
    /// sent to the sender.
    pub async fn handle_with<Func, Fut>(&self, func: Func) -> Result<()>
    where
        Fut: Future<Output = Result<()>> + Send,
        Func: FnOnce(Ctx<'a, Loaded>) -> Fut + Send,
    {
        let ctx = self.clone();
        let mut loaded = ctx.upgrade().await?;

        // Error occurred in inner will be sent to user directly - Logic error
        let inner = move || async {
            loaded.assert_in_group()?;
            loaded.fetch_real_chat_member().await?;
            func(loaded).await?;
            Result::<()>::Ok(())
        };

        if let Err(e) = inner().await {
            self.reply_to_then_del(e.to_string()).await?;
        }
        self.del_msg_delayed();

        Ok(())
    }
}

impl<'a, S> Ctx<'a, S> {
    /// Get the bot reference
    #[inline]
    #[must_use]
    pub const fn bot(&self) -> &BotType {
        self.bot
    }

    /// Get the msg reference
    #[inline]
    #[must_use]
    pub const fn msg(&self) -> &Message {
        self.msg
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
    pub const fn sender(&self) -> &User {
        &self.sender
    }

    #[inline]
    #[must_use]
    pub const fn is_anonymous(&self) -> bool {
        self.is_anonymous
    }

    /// Get the [`UserId`] of current sender
    #[inline]
    #[must_use]
    pub const fn sender_id(&self) -> UserId {
        self.sender().id
    }

    /// Save the title record to db
    ///
    /// # Errors
    /// When unable to save to db
    fn save_title(&self, title: &str) -> Result<()> {
        let record = TitleRecord {
            chat_id: self.chat_id(),
            user_id: self.sender_id(),
            title: title.into(),
        };

        record.insert_into(self.db)?;

        Ok(())
    }

    /// Set title of user
    ///
    /// # Errors
    /// If the user cannot be set a title or requesting error.
    pub async fn set_title(&self, title: impl Into<String> + Send) -> Result<()> {
        let title = title.into();
        let existing = self.get_record_with_sig(&title)?;
        ensure!(existing.is_none(), "Title already in use");
        self.remove_title_with_id()?;
        self.bot
            .set_chat_administrator_custom_title(self.chat_id(), self.sender_id(), &title)
            .await
            .map_err(|error| {
                send_debug(&error);
                eyre!("Failed to set title")
            })?;
        self.save_title(&title)?;
        Ok(())
    }

    /// Get the all titles in current chat
    ///
    /// # Errors
    /// If the database returns an error or the data is not in good shape.
    pub fn list_titles(&self) -> Result<Vec<TitleRecord>> {
        TitleRecord::list_in_chat(self.db, self.chat_id())
    }

    /// Remove the given title from db with signature
    ///
    /// # Errors
    /// When unable to remove from db
    pub fn remove_title_with_sig(&self, sig: &str) -> Result<()> {
        let existing = self.get_record_with_sig(sig)?;
        match existing {
            None => Ok(()),
            Some(existing) => existing.remove_from(self.db),
        }
    }

    /// Remove the given title from db with id
    ///
    /// # Errors
    /// When unable to remove from db
    pub fn remove_title_with_id(&self) -> Result<()> {
        let existing = self.get_record_with_id()?;
        match existing {
            None => Ok(()),
            Some(existing) => existing.remove_from(self.db),
        }
    }

    /// Retrieve the title record with current user id and chat id
    ///
    /// # Errors
    /// When db returns an error or the title is not UTF-8
    pub fn get_record_with_id(&self) -> Result<Option<TitleRecord>> {
        TitleRecord::get_with_id(self.db, self.chat_id(), self.sender_id())
    }

    /// Retrieve title record with `author_signature`, which is the tile of
    /// anonymouse admins.
    ///
    /// # Errors
    /// When db returns an error or the title is not UTF-8
    pub fn get_record_with_sig(&self, sig: &str) -> Result<Option<TitleRecord>> {
        TitleRecord::get_with_title(self.db, self.chat_id(), sig)
    }

    /// Fetches the conversation information from the bot and turn self into
    /// [`Full`].
    ///
    /// # Errors
    /// If the chat member information cannot be fetched.
    async fn upgrade(self) -> Result<Ctx<'a, Loaded>> {
        let (me, sender) = try_join!(
            self.bot.get_chat_member(
                self.chat_id(),
                BOT_INFO.get().expect("Bot info not initialized").0
            ),
            self.bot.get_chat_member(self.chat_id(), self.sender_id())
        )
        .tap_err(|error| {
            send_debug(error);
        })?;

        let Self { bot, msg, db, .. } = self;

        Ok(Ctx {
            bot,
            msg,
            db,
            sender: sender.user.clone(),
            is_anonymous: false,
            conversation: Loaded::new(me, sender),
        })
    }

    /// Find specific admin in the current chat
    ///
    /// # Errors
    /// API errors
    pub async fn find_admin_with_username(&self, username: &str) -> Result<Option<ChatMember>> {
        Ok(self
            .bot
            .get_chat_administrators(self.chat_id())
            .await?
            .into_iter()
            .find(|user| user.user.username.as_deref() == Some(username)))
    }

    /// Demote everyone and remove all titles in chat
    ///
    /// # Errors
    /// If the bot cannot demote everyone or the database cannot remove all
    pub async fn nuke(&self) -> Result<()> {
        let chat_id = self.chat_id();

        let all_admins = self
            .bot
            .get_chat_administrators(self.chat_id())
            .await
            .map_err(|e| {
                send_debug(&e);
                eyre!("Failed to load all admins")
            })?;

        let all_count = all_admins.len() - 1;

        let res = try_join_all(
            all_admins
                .into_iter()
                .filter(|x| x.is_administrator() && x.can_be_edited())
                .map(|member| {
                    let id = member.user.id;
                    if let Some(record) = TitleRecord::get_with_id(self.db, chat_id, id)? {
                        record.remove_from(self.db)?;
                    };
                    let fut = async move {
                        self.bot.promote_chat_member(chat_id, id).send().await?;

                        Result::<_>::Ok(())
                    };
                    Result::<_>::Ok(fut)
                })
                .try_collect::<Vec<_>>()?,
        )
        .await
        .map_err(|e| {
            send_debug(&e);
            eyre!("Failed to load remove all admins")
        })?;

        self.reply_to(format!("Found {} admins, demoted {}", all_count, res.len()))
            .await?;

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
                send_debug(&error);
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
            .map_err(|error| {
                send_debug(&error);
                eyre!("Promote member error")
            })?;
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
            .map_err(|error| {
                send_debug(&error);
                eyre!("Demote member error")
            })?;
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

    /// Reply to the sender with a message and delete the msg after a period of
    /// time.
    ///
    /// # Errors
    /// When fails to send the message.
    pub async fn reply_to_then_del(&self, text: impl Into<String> + Send) -> Result<()> {
        let msg = self
            .bot
            .send_message(self.chat_id(), text)
            .reply_to_message_id(self.msg.id)
            .await?;
        self.del_msg_delayed_with_id(msg.id);
        Ok(())
    }

    pub fn del_msg_delayed(&self) {
        self.del_msg_delayed_with_id(self.msg.id);
    }

    /// Delete the message with the given id after a period of time.
    ///
    /// # Panics
    /// If either bot or config is not initialized.
    pub fn del_msg_delayed_with_id(&self, msg_id: i32) {
        let chat_id = self.chat_id();

        tokio::spawn(async move {
            let config = Config::get();
            tokio::time::sleep(config.delete_after).await;
            let bot = BOT.get().unwrap();
            catch!(bot.delete_message(chat_id, msg_id).send().await);
        });
    }

    /// Delete the command.
    ///
    /// # Errors
    /// If the bot cannot delete the message.
    pub async fn del_msg(&self) -> Result<()> {
        let chat_id = self.chat_id();
        self.bot.delete_message(chat_id, self.msg.id).send().await?;
        Ok(())
    }

    /// Tell the sender that the requested action has been conducted.
    ///
    /// # Errors
    /// When the message deletion failed.
    pub async fn done(&self) -> Result<()> {
        self.reply_to_then_del("Done! Wait for a while to take effect.")
            .await
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

impl<'a, 'u> Ctx<'a, Loaded> {
    #[inline]
    #[must_use]
    pub const fn me_in_chat(&self) -> &ChatMember {
        &self.conversation.me
    }

    #[inline]
    #[must_use]
    pub const fn sender_in_chat(&self) -> &ChatMember {
        &self.conversation.sender
    }

    /// Create and enter a temporary scope with ctx with given [`ChatMember`] as
    /// the sender.
    ///
    /// # Errors
    /// If the function returns an error.
    pub async fn with_sender<Func, Fut>(&mut self, sender: ChatMember, func: Func) -> Result<()>
    where
        Fut: Future<Output = Result<()>>,
        Func: FnOnce(Ctx<'a, Loaded>) -> Fut,
    {
        let temp = Self {
            bot: self.bot,
            db: self.db,
            msg: self.msg,
            sender: sender.user.clone(),
            conversation: Loaded::new(self.me_in_chat().clone(), sender),
            is_anonymous: self.is_anonymous,
        };

        func(temp).await?;
        Ok(())
    }

    /// If sender is anonymous, try find real sender
    ///
    /// # Errors
    /// If the sender is not found or error during fetching
    pub async fn fetch_real_chat_member(&mut self) -> Result<()> {
        // Sender is anonymous, try to decode the identity
        if self.conversation.sender.user.first_name == "Group" {
            info!("Sender is anonymous, trying to find real identity");
            self.is_anonymous = true;
            let sig = match self.msg.author_signature() {
                Some(sig) => sig,
                None => {
                    bail!("Unable to identify target (no title)")
                }
            };
            let real = match self.get_record_with_sig(sig)? {
                Some(real) => real,
                None => bail!("Unable to identify target (no record found)"),
            };
            let real = self.bot.get_chat_member(real.chat_id, real.user_id).await?;
            self.sender = real.user.clone();
            self.conversation.sender = real.into();
        }
        Ok(())
    }

    /// Prepare for editing user privilege
    ///
    /// This will check for proper privileges according to status of the
    /// conversation.
    ///
    /// # Errors
    ///
    /// If the bot or the user is not privileged enough or suitable to promote
    /// or be promoted.
    pub async fn prep_edit(&self) -> Result<()> {
        use ChatMemberKind::*;

        match &self.sender_in_chat().kind {
            Administrator(_) => self.assert_editable()?,
            Member => {
                self.assert_bot_promotable()?;
                self.promote().await.map_err(|error| {
                    send_debug(&error);
                    eyre!("Failed to promote")
                })?;
                self.reply_to("Promoted, wait...").await?;
                // Wait a while for the promotion to take effect.
                sleep(Duration::from_secs_f32(1.5)).await;
            }
            kind => bail!(
                "Unable to edit you/them because of your(their) status({})",
                chat_member_kind_to_str(kind)
            ),
        }
        Ok(())
    }

    /// De-anonymous user
    ///
    /// # Errors
    /// When user not found or error during interaction with tg api
    pub async fn de_anonymous(&self) -> Result<()> {
        self.assert_sender_anonymous()?;

        self.bot
            .promote_chat_member(self.chat_id(), self.sender_in_chat().user.id)
            .can_invite_users(true)
            .send()
            .await
            .map_err(|error| {
                send_debug(&error);
                eyre!("Set privilege error")
            })?;

        Ok(())
    }

    /// Ensure that the bot is an admin in the chat.
    ///
    /// # Errors
    /// Failed when not an admin.
    pub fn assert_bot_admin(&self) -> Result<()> {
        match &self.me_in_chat().kind {
            ChatMemberKind::Owner(_) | ChatMemberKind::Administrator(_) => Ok(()),
            kind => bail!(
                "I am not an admin, please contact admin (Currently {})",
                chat_member_kind_to_str(kind)
            ),
        }
    }

    /// Ensure that the sender is an admin in the chat.
    ///
    /// # Errors
    /// Failed when not an admin.
    pub fn assert_sender_admin(&self) -> Result<()> {
        match &self.sender_in_chat().kind {
            ChatMemberKind::Owner(_) | ChatMemberKind::Administrator(_) => Ok(()),
            kind => bail!(
                "You/they are not admin, please contact admin (Currently {})",
                chat_member_kind_to_str(kind)
            ),
        }
    }

    /// Ensure that the sender is the owner of the chat.
    ///
    /// # Errors
    /// Failed when not an admin.
    pub fn assert_sender_owner(&self) -> Result<()> {
        match &self.sender_in_chat().kind {
            ChatMemberKind::Owner(_) => Ok(()),
            kind => bail!(
                "This function is owner only, (you/they are {})",
                chat_member_kind_to_str(kind)
            ),
        }
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
                        "Unable to change info (maybe promoted by others?)"
                    );
                    Ok(())
                }
                Member => Ok(()),
                ref k => bail!(
                    "Unable to edit because of status ({})",
                    chat_member_kind_to_str(k)
                ),
            },
            _ => bail!("I'm not an admin, please promote me with promotion privilege first"),
        }
    }

    /// Ensure that the sender is privileged enough to promote the user.
    ///
    /// # Errors
    /// Failed when not privileged enough.
    pub fn assert_bot_promotable(&self) -> Result<()> {
        let kind = &self.me_in_chat().kind;

        ensure!(
            kind.can_promote_members() && kind.can_invite_users(),
            "Unable to promote others because lack of privilege"
        );

        Ok(())
    }

    /// Ensure that the bot is admin & anonymous.
    ///
    /// # Errors
    /// If the privilege and status are not fullfilled.
    pub fn assert_bot_anonymous(&self) -> Result<()> {
        let kind = &self.me_in_chat().kind;

        ensure!(
            kind.can_promote_members() && kind.is_anonymous(),
            "Unable to make others anonymous because lack of privilege"
        );

        Ok(())
    }

    /// Ensure that the sender is admin & anonymous.
    ///
    /// # Errors
    /// If the privilege and status are not fullfilled.
    #[allow(clippy::missing_panics_doc)]
    pub fn assert_sender_anonymous(&self) -> Result<()> {
        ensure!(self.is_anonymous, "Target not anonymous");
        Ok(())
    }
}

#[must_use]
pub const fn chat_member_kind_to_str(kind: &ChatMemberKind) -> &'static str {
    use ChatMemberKind::*;

    match kind {
        Administrator(..) => "admin",
        Member => "member",
        Owner(_) => "owner",
        Restricted(_) => "restricted",
        Left => "left",
        Banned(_) => "banned",
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleRecord {
    pub title: String,
    pub chat_id: ChatId,
    pub user_id: UserId,
}

impl TitleRecord {
    fn list_in_chat(db: &Db, chat: ChatId) -> Result<Vec<Self>> {
        let prefix = format!("chat${}", chat);
        db.scan_prefix(&prefix)
            .map(|x| {
                x.wrap_err("Failed to scan database")
                    .and_then(|(key, value)| Self::parse_chat_key(&key, &value))
            })
            .try_collect()
    }

    /// Insert given record into DB
    ///
    /// # Errors
    /// If the insertion fails.
    fn insert_into(&self, db: &Db) -> Result<()> {
        let chat_key: IVec = Self::make_chat_key(self.chat_id, self.user_id);
        let title_key: IVec = Self::make_title_key(self.chat_id, &self.title);

        db.insert(&chat_key, self.title.as_bytes())?;
        db.insert(&title_key, &self.user_id.0.to_be_bytes())?;

        Ok(())
    }

    /// Get the record from DB with `chat_id` and `user_id`.
    /// Note: Do not get record with id when user is anonymous, since the id is
    /// hidden by Telegram. Use `get_by_title` with `author_signature`
    /// instead.
    ///
    /// # Errors
    /// When get fails or bad encoding.
    fn get_with_id(db: &Db, chat_id: ChatId, user_id: UserId) -> Result<Option<Self>> {
        let chat_key: IVec = Self::make_chat_key(chat_id, user_id);

        let title = match db.get(chat_key)? {
            Some(title_key) => String::from_utf8(title_key.to_vec())?,
            None => return Ok(None),
        };

        Ok(Some(Self {
            title,
            chat_id,
            user_id,
        }))
    }

    /// Get the record from DB with `title`
    ///
    /// # Errors
    /// When get fails or bad encoding.
    fn get_with_title(db: &Db, chat_id: ChatId, title: impl Into<String>) -> Result<Option<Self>> {
        let title = title.into();

        let title_key: IVec = Self::make_title_key(chat_id, &title);
        let user_id = match db.get(title_key)? {
            Some(chat_key) => u64::from_be_bytes((*chat_key).try_into().wrap_err("Bad value")?),
            None => return Ok(None),
        };

        Ok(Some(Self {
            title,
            chat_id,
            user_id: UserId(user_id),
        }))
    }

    fn remove_from(&self, db: &Db) -> Result<()> {
        let chat_key: IVec = Self::make_chat_key(self.chat_id, self.user_id);
        let title_key: IVec = Self::make_title_key(self.chat_id, &self.title);
        db.remove(title_key)?;
        db.remove(chat_key)?;
        Ok(())
    }

    fn make_title_key(chat_id: ChatId, title: &str) -> IVec {
        format!("title${}${}", chat_id, title).into_bytes().into()
    }

    fn make_chat_key(chat_id: ChatId, user_id: UserId) -> IVec {
        format!("chat${}${}", chat_id, user_id).into_bytes().into()
    }

    fn parse_chat_key(key: &IVec, title: &IVec) -> Result<Self> {
        let key = String::from_utf8(key.to_vec())?;
        let mut iter = key.split('$');

        ensure!(iter.next() == Some("chat"), "Bad key");

        let chat_id = iter
            .next()
            .wrap_err("bad key")?
            .parse::<i64>()
            .map(ChatId)?;
        let user_id = iter
            .next()
            .wrap_err("bad key")?
            .parse::<u64>()
            .map(UserId)?;

        let title = String::from_utf8(title.to_vec())?;

        Ok(Self {
            title,
            chat_id,
            user_id,
        })
    }
}

impl Display for TitleRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<code>{}: User({})</code>", self.title, self.user_id)
    }
}

#[test]
fn test_db() {
    let db = sled::open("/tmp/test_db").unwrap();

    let record = TitleRecord {
        title: "test".into(),
        chat_id: ChatId(1),
        user_id: UserId(2),
    };

    record.insert_into(&db).unwrap();

    let record2 = TitleRecord::get_with_id(&db, ChatId(1), UserId(2))
        .unwrap()
        .unwrap();
    assert_eq!(record, record2);

    let record3 = TitleRecord::get_with_title(&db, ChatId(1), "test")
        .unwrap()
        .unwrap();
    assert_eq!(record, record3);

    record.remove_from(&db).unwrap();
    assert_eq!(
        TitleRecord::get_with_id(&db, ChatId(1), UserId(2)).unwrap(),
        None
    );
}

#[test]
fn test_list_db() {
    let db = sled::open("/tmp/test_db").unwrap();

    let r0 = TitleRecord {
        title: "test".into(),
        chat_id: ChatId(1),
        user_id: UserId(2),
    };

    let r1 = TitleRecord {
        title: "test".into(),
        chat_id: ChatId(1),
        user_id: UserId(3),
    };

    let r2 = TitleRecord {
        title: "test".into(),
        chat_id: ChatId(1),
        user_id: UserId(4),
    };

    r0.insert_into(&db).unwrap();
    r1.insert_into(&db).unwrap();
    r2.insert_into(&db).unwrap();

    let records = TitleRecord::list_in_chat(&db, ChatId(1)).unwrap();
    let empty = TitleRecord::list_in_chat(&db, ChatId(114_514)).unwrap();
    assert_eq!(records, vec![r0, r1, r2]);
    assert!(empty.is_empty());
}
