use std::sync::{Arc, LazyLock, RwLock};

use crate::{
    config::{self, EmailNoticeConfig},
    db,
    model::entity::rule,
};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use log::{error, info};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

/// 发送邮件
pub(crate) async fn send_email(config: EmailNoticeConfig, user_id: &str) -> anyhow::Result<()> {
    let from = format!("Bot <{}>", config.username);
    let to = format!("Master <{}>", config.receiver);
    let email_template = config.mail.clone().unwrap_or_default();
    let body = replace_placeholders(&email_template.body, &[("bot_id", user_id)]);

    let email = Message::builder()
        .from(from.parse()?)
        .to(to.parse()?)
        .subject(email_template.subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)?;

    let creds = Credentials::new(config.username.clone(), config.password.clone());

    let mailer: AsyncSmtpTransport<Tokio1Executor> = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp)?
        .credentials(creds)
        .build();

    mailer.send(email).await?;

    Ok(())
}

fn replace_placeholders(text: &str, map: &[(&str, &str)]) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    let mut placeholder_start = None;

    for (i, c) in text.char_indices() {
        match c {
            '{' => placeholder_start = Some(i),
            '}' => {
                if let Some(start) = placeholder_start {
                    let key = &text[start + 1..i];
                    if let Some((_, value)) = map.iter().find(|&&(k, _)| k == key) {
                        result.push_str(&text[last_end..start]);
                        result.push_str(value);
                        last_end = i + 1;
                    }
                    placeholder_start = None;
                }
            }
            _ => {}
        }
    }
    result.push_str(&text[last_end..]);
    result
}

/// 群组规则
struct GroupInfo {
    /// 群组ID
    id: i64,
    /// 被排除在外的用户
    exclude_user: Vec<i64>,
}

pub enum ReloadType {
    WhileGroup,
    BlackGroup,
    WhileUser,
    BlackUser,
}

type GroupInfoList = Arc<RwLock<Vec<GroupInfo>>>;
type UserList = Arc<RwLock<Vec<i64>>>;

static WHITE_GROUP_LIST: LazyLock<GroupInfoList> = LazyLock::new(|| Arc::new(RwLock::new(Vec::new())));
static BLACK_GROUP_LIST: LazyLock<GroupInfoList> = LazyLock::new(|| Arc::new(RwLock::new(Vec::new())));
static WHITE_USER_LIST: LazyLock<UserList> = LazyLock::new(|| Arc::new(RwLock::new(Vec::new())));
static BLACK_USER_LIST: LazyLock<UserList> = LazyLock::new(|| Arc::new(RwLock::new(Vec::new())));

pub struct DatabaseCache;

impl DatabaseCache {
    pub async fn load() {
        // 初始化白名单和黑名单
        if let Err(err) = Self::reload_database_data(vec![
            ReloadType::WhileGroup,
            ReloadType::BlackGroup,
            ReloadType::WhileUser,
            ReloadType::BlackUser,
        ])
        .await
        {
            error!("Failed to reload database data: {}", err);
        }
    }

    /// 重新加载数据库数据
    pub async fn reload_database_data(types: Vec<ReloadType>) -> anyhow::Result<()> {
        for r#type in types {
            match r#type {
                ReloadType::WhileGroup => {
                    let rules = rule::Entity::find()
                        .filter(rule::Column::ChatType.eq(rule::ChatType::Group))
                        .filter(rule::Column::ItemType.eq(rule::ItemType::WhiteList))
                        .all(db!())
                        .await?;
                    let mut result = Vec::new();
                    for id in rules.iter().map(|rule| rule.chat_id) {
                        let exclude_user = rule::Entity::find()
                            .filter(rule::Column::ChatType.eq(rule::ChatType::User))
                            .filter(rule::Column::ItemType.eq(rule::ItemType::BlackList))
                            .filter(rule::Column::GroupChatId.eq(Some(id)))
                            .all(db!())
                            .await?
                            .iter()
                            .map(|rule| rule.chat_id)
                            .collect::<Vec<_>>();
                        result.push(GroupInfo { id, exclude_user });
                    }
                    *WHITE_GROUP_LIST.write().unwrap() = result;
                }
                ReloadType::BlackGroup => {
                    let rules = rule::Entity::find()
                        .filter(rule::Column::ChatType.eq(rule::ChatType::Group))
                        .filter(rule::Column::ItemType.eq(rule::ItemType::BlackList))
                        .all(db!())
                        .await?;
                    let mut result = Vec::new();
                    for id in rules.iter().map(|rule| rule.chat_id) {
                        let exclude_user = rule::Entity::find()
                            .filter(rule::Column::ChatType.eq(rule::ChatType::User))
                            .filter(rule::Column::ItemType.eq(rule::ItemType::WhiteList))
                            .filter(rule::Column::GroupChatId.eq(Some(id)))
                            .all(db!())
                            .await?
                            .iter()
                            .map(|rule| rule.chat_id)
                            .collect::<Vec<_>>();
                        result.push(GroupInfo { id, exclude_user });
                    }
                    *BLACK_GROUP_LIST.write().unwrap() = result;
                }
                ReloadType::WhileUser => {
                    let rules = rule::Entity::find()
                        .filter(rule::Column::ChatType.eq(rule::ChatType::User))
                        .filter(rule::Column::ItemType.eq(rule::ItemType::WhiteList))
                        .filter(rule::Column::GroupChatId.eq(None::<i64>))
                        .all(db!())
                        .await?;
                    let result = rules.iter().map(|rule| rule.chat_id).collect();
                    *WHITE_USER_LIST.write().unwrap() = result;
                }
                ReloadType::BlackUser => {
                    let rules = rule::Entity::find()
                        .filter(rule::Column::ChatType.eq(rule::ChatType::User))
                        .filter(rule::Column::ItemType.eq(rule::ItemType::BlackList))
                        .filter(rule::Column::GroupChatId.eq(None::<i64>))
                        .all(db!())
                        .await?;
                    let result = rules.iter().map(|rule| rule.chat_id).collect();
                    *BLACK_USER_LIST.write().unwrap() = result;
                }
            }
        }

        Ok(())
    }
    /// 判断消息是否应该放行
    pub async fn send_by_auth(group_id: Option<i64>, user_id: Option<i64>) -> bool {
        let default_policy = config::APP_CONFIG.clone().default_policy.clone();
        if let Some(user_id) = user_id {
            if BLACK_USER_LIST.read().unwrap().contains(&user_id) {
                return false;
            }
            if WHITE_USER_LIST.read().unwrap().contains(&user_id) {
                return true;
            }
        }
        if let Some(group_id) = group_id {
            // 判断群组白名单
            for group in &*WHITE_GROUP_LIST.read().unwrap() {
                if group.id == group_id {
                    if let Some(user_id) = user_id {
                        if group.exclude_user.contains(&user_id) || BLACK_USER_LIST.read().unwrap().contains(&user_id) {
                            info!("group {} is in whilelist, but user {} was exclude", group_id, user_id);
                            return false;
                        } else {
                            info!("group {} is in whitelist, send message", group_id);
                            return true;
                        }
                    } else {
                        info!("group {} is in whitelist, send message", group_id);
                        return true;
                    }
                }
            }
            for group in &*BLACK_GROUP_LIST.read().unwrap() {
                if group.id == group_id {
                    if let Some(user_id) = user_id {
                        if group.exclude_user.contains(&user_id) || WHITE_USER_LIST.read().unwrap().contains(&user_id) {
                            info!("group {} is in blacklist, but user {} was exclude", group_id, user_id);
                            return true;
                        } else {
                            info!("group {} is in blacklist, send message", group_id);
                            return false;
                        }
                    } else {
                        info!("group {} is in blacklist, send message", group_id);
                        return false;
                    }
                }
            }
        }
        matches!(default_policy.unwrap_or_default(), config::Policy::Allow)
    }
}

#[macro_export]
macro_rules! ctrl_c_signal {
    () => {
        tokio::signal::ctrl_c()
    };
}
