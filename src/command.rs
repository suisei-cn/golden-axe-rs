use anyhow::{anyhow, bail, Result};
use teloxide::payloads::PromoteChatMember;
use teloxide::prelude::*;
use teloxide::requests::JsonRequest;
use teloxide::types::{ChatKind, ChatMemberKind, ChatPublic, Message, PublicChatKind};
use teloxide::utils::command::BotCommand;
use teloxide::Bot;

#[derive(BotCommand)]
#[command(rename = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "display this text.")]
    Help,
    #[command(description = "change my title.")]
    Title { title: String },
}

pub async fn handle_command(
    cx: UpdateWithCx<AutoSend<Bot>, Message>,
    command: Command,
) -> Result<()> {
    match command {
        Command::Help => {
            cx.answer(Command::descriptions()).await?;
        }
        Command::Title { title } => set_title(cx, title).await?,
    };

    Ok(())
}

async fn set_title(cx: UpdateWithCx<AutoSend<Bot>, Message>, title: String) -> Result<()> {
    if title.is_empty() {
        cx.answer("Title cannot be empty").await?;
        bail!("Bad request: empty title");
    }
    let sender = cx.update.from().ok_or(anyhow!("No sender"))?;

    let chat_id = cx.chat_id();
    match cx.update.chat.kind {
        ChatKind::Public(ChatPublic {
            kind: PublicChatKind::Group(_) | PublicChatKind::Supergroup(_),
            ..
        }) => {}
        _ => {
            cx.reply_to("Call this command in a group or supergroup")
                .await?;
            bail!("Bad request: not in group")
        }
    };
    let bot = cx.requester.clone();
    let me_in_chat = bot
        .get_chat_member(chat_id, bot.get_me().await?.user.id)
        .await?;
    let sender_in_chat = bot.get_chat_member(chat_id, sender.id).await?;

    match me_in_chat.kind {
        // The bot has rights to promote members
        ChatMemberKind::Administrator(my_rights) if my_rights.can_promote_members => {
            match sender_in_chat.kind {
                // The bot can edit the member
                ChatMemberKind::Administrator(sender_rights) if sender_rights.can_be_edited => {
                    bot.set_chat_administrator_custom_title(chat_id, sender.id, title)
                        .await?;
                    cx.reply_to("Done!").await?;
                }

                // The member is admin, but the bot can't edit him (others promotes him)
                ChatMemberKind::Administrator(_) => {
                    cx.reply_to("Failed: I can't change your info (are you promoted by others?)")
                        .await?;
                }

                // The member is normal member
                ChatMemberKind::Member => {
                    let payload = PromoteChatMember {
                        chat_id: chat_id.into(),
                        user_id: sender.id,
                        can_invite_users: Some(true),
                        can_manage_chat: None,
                        can_change_info: None,
                        can_post_messages: None,
                        can_edit_messages: None,
                        can_delete_messages: None,
                        can_manage_voice_chats: None,
                        can_restrict_members: None,
                        can_pin_messages: None,
                        can_promote_members: None,
                        is_anonymous: None,
                    };

                    JsonRequest::new(bot.inner().clone(), payload)
                        .send()
                        .await?;
                    bot.set_chat_administrator_custom_title(chat_id, sender.id, title)
                        .await?;
                    cx.reply_to("Done!").await?;
                }

                // Other
                _ => {
                    cx.reply_to("I can't change your info").await?;
                }
            }
        }
        // The bot is admin but don't have the privilege to promote admins
        ChatMemberKind::Administrator(_) => {
            cx.reply_to("I don't have privilege to promote members")
                .await?;
        }
        // Other
        _ => {
            cx.reply_to("I'm not an admin").await?;
        }
    }
    Ok(())
}
