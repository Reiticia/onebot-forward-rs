use clap::{Parser, Subcommand};
use log::info;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

use crate::model::config;
use crate::model::entity::rule;
use crate::model::entity::rule::ItemType;

// 顶层命令行结构
#[derive(Parser, Debug)]
#[clap(no_binary_name = true)] // 忽略默认的二进制名称
pub(crate) struct Cli {
    #[clap(subcommand)]
    command: Commands, // 子命令：黑名单或白名单
}

// 子命令枚举
#[derive(Subcommand, Debug)]
enum Commands {
    #[clap(name = "黑名单")]
    Blacklist {
        #[clap(subcommand)]
        action: Action,
    }, // 黑名单操作
    #[clap(name = "白名单")]
    Whitelist {
        #[clap(subcommand)]
        action: Action,
    }, // 白名单操作
}

// 具体操作（创建、删除、列表）
#[derive(Subcommand, Debug)]
enum Action {
    #[clap(name = "创建", alias = "新增")]
    Create { id: i64 },
    #[clap(name = "删除", alias = "移除")]
    Delete { id: i64 },
    #[clap(name = "列表", alias = "查询")]
    List,
}

/// 反馈信息给协议端的结构
pub struct Response {
    pub action: String,
    pub success: bool,
    pub data: String,
}

impl Cli {
    pub fn parse_command(command: &str) -> anyhow::Result<Self> {
        let args: Vec<&str> = command.split_whitespace().collect();
        Cli::try_parse_from(args).map_err(|err| anyhow::anyhow!(err.to_string()))
    }

    pub async fn execute(&self) -> anyhow::Result<Response> {
        match &self.command {
            Commands::Blacklist { action } => Self::handle_action(action, ItemType::BlackList).await,
            Commands::Whitelist { action } => Self::handle_action(action, ItemType::WhiteList).await,
        }
    }

    /// 处理具体的操作
    async fn handle_action(action: &Action, item_type: ItemType) -> anyhow::Result<Response> {
        let db = config::APP_CONFIG_DB
            .get()
            .ok_or_else(|| anyhow::anyhow!("数据库连接未初始化"))?;
        let action_str = match item_type {
            ItemType::BlackList => "黑名单",
            ItemType::WhiteList => "白名单",
        };
        match action {
            Action::Create { id } => {
                // 创建操作的逻辑
                let rule = rule::ActiveModel {
                    id: sea_orm::ActiveValue::NotSet,
                    chat_id: sea_orm::ActiveValue::Set(*id),
                    chat_type: sea_orm::ActiveValue::Set(rule::ChatType::Group),
                    item_type: sea_orm::ActiveValue::Set(item_type),
                    created_at: sea_orm::ActiveValue::Set(chrono::Utc::now().into()),
                    updated_at: sea_orm::ActiveValue::Set(chrono::Utc::now().into()),
                };
                if rule::Entity::find()
                    .filter(rule::Column::ChatId.eq(*id))
                    .filter(rule::Column::ItemType.eq(item_type))
                    .one(db)
                    .await?
                    .is_some()
                {
                    let message = Response {
                        action: format!("添加{}", action_str),
                        success: false,
                        data: "已存在".into(),
                    };
                    return Ok(message);
                }
                rule.save(db).await?;
                info!("{:?} 添加 {} 成功", item_type, id);
                let message = Response {
                    action: format!("添加{}", action_str),
                    success: true,
                    data: id.to_string(),
                };
                Ok(message)
            }
            Action::Delete { id } => {
                // 删除操作的逻辑
                let row_affected = rule::Entity::delete_many()
                    .filter(rule::Column::ChatId.eq(*id))
                    .filter(rule::Column::ItemType.eq(item_type))
                    .exec(db)
                    .await?
                    .rows_affected;
                if row_affected == 0 {
                    let message = Response {
                        action: format!("删除{}", action_str),
                        success: false,
                        data: "记录不存在".into(),
                    };
                    return Ok(message);
                }
                info!("{:?} 删除 {} 成功", item_type, id);
                let message = Response {
                    action: format!("删除{}", action_str),
                    success: true,
                    data: id.to_string(),
                };
                Ok(message)
            }
            Action::List => {
                // 列表操作的逻辑
                let rules = rule::Entity::find()
                    .filter(rule::Column::ItemType.eq(item_type))
                    .all(db)
                    .await?;
                let rules: Vec<i64> = rules.iter().map(|rule| rule.chat_id).collect::<Vec<_>>();
                let message = Response {
                    action: format!("获取{}列表", action_str),
                    success: true,
                    data: format!("{:?}", rules),
                };
                Ok(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command() {
        let command = "黑名单 创建 123";
        let cli = Cli::parse_command(command);
        println!("{:?}", cli);
    }

    #[test]
    fn test_parse_command_error() {
        let command = "黑名单 新增 123";
        let cli = Cli::parse_command(command);
        println!("{:?}", cli);
    }
}
