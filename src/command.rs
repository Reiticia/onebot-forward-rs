use clap::{Parser, Subcommand};
use log::error;
use log::info;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;

use crate::db;
use crate::model::entity::rule;
use crate::model::entity::rule::ItemType;
use crate::utils::ReloadType;
use crate::utils::reload_database_data;

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
    #[clap(name = "添加群", alias = "新增群")]
    CreateGroup { id: i64 },
    #[clap(name = "删除群", alias = "移除群")]
    DeleteGroup { id: i64 },
    #[clap(name = "列表群", alias = "查询群")]
    ListGroup,
    #[clap(name = "添加人", alias = "新增人")]
    CreateUser {
        id: i64,
        #[arg(short)]
        group_id: Option<i64>,
    },
    #[clap(name = "删除人", alias = "移除人")]
    DeleteUser {
        id: i64,
        #[arg(short)]
        group_id: Option<i64>,
    },
    #[clap(name = "列表人", alias = "查询人")]
    ListUser {
        #[arg(short)]
        group_id: Option<i64>,
    },
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
        let action_str = match item_type {
            ItemType::BlackList => "黑名单",
            ItemType::WhiteList => "白名单",
        };
        let response = match action {
            Action::CreateGroup { id } => Self::create_group(*id, item_type, action_str).await,
            Action::DeleteGroup { id } => Self::delete_group(*id, item_type, action_str).await,
            Action::ListGroup => Self::list_group(item_type, action_str).await,
            Action::CreateUser { id, group_id } => Self::create_user(*id, *group_id, item_type, action_str).await,
            Action::DeleteUser { id, group_id } => Self::delete_user(*id, *group_id, item_type, action_str).await,
            Action::ListUser { group_id } => Self::list_user(*group_id, item_type, action_str).await,
        };

        let reload_type = match action {
            Action::CreateGroup { .. } | Action::DeleteGroup { .. } => match item_type {
                ItemType::BlackList => vec![ReloadType::BlackGroup],
                ItemType::WhiteList => vec![ReloadType::WhileGroup],
            },
            Action::CreateUser { .. } | Action::DeleteUser { .. } => match item_type {
                ItemType::BlackList => vec![ReloadType::WhileGroup, ReloadType::BlackUser],
                ItemType::WhiteList => vec![ReloadType::BlackGroup, ReloadType::WhileUser],
            },
            _ => match item_type {
                ItemType::BlackList => vec![],
                ItemType::WhiteList => vec![],
            },
        };

        if let Err(err) = reload_database_data(reload_type).await {
            error!("database execute error {:?}", err);
        }

        response
    }

    /// 添加群
    async fn create_group(id: i64, item_type: ItemType, action_str: &str) -> anyhow::Result<Response> {
        // 创建操作的逻辑
        let rule = rule::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            chat_id: sea_orm::ActiveValue::Set(id),
            chat_type: sea_orm::ActiveValue::Set(rule::ChatType::Group),
            item_type: sea_orm::ActiveValue::Set(item_type),
            created_at: sea_orm::ActiveValue::Set(chrono::Utc::now().into()),
            updated_at: sea_orm::ActiveValue::Set(chrono::Utc::now().into()),
            group_chat_id: sea_orm::ActiveValue::Set(None),
        };
        if rule::Entity::find()
            .filter(rule::Column::ChatId.eq(id))
            .filter(rule::Column::ItemType.eq(item_type))
            .one(db!())
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
        rule.save(db!()).await?;
        info!("{:?} add {} success", item_type, id);
        let message = Response {
            action: format!("添加{}", action_str),
            success: true,
            data: id.to_string(),
        };
        Ok(message)
    }

    /// 删除群
    async fn delete_group(id: i64, item_type: ItemType, action_str: &str) -> anyhow::Result<Response> {
        // 删除操作的逻辑
        let row_affected = rule::Entity::delete_many()
            .filter(rule::Column::ChatId.eq(id))
            .filter(rule::Column::ItemType.eq(item_type))
            .exec(db!())
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
        info!("{:?} remove {} success", item_type, id);
        let message = Response {
            action: format!("删除{}", action_str),
            success: true,
            data: id.to_string(),
        };
        Ok(message)
    }

    /// 列表群
    async fn list_group(item_type: ItemType, action_str: &str) -> anyhow::Result<Response> {
        // 列表操作的逻辑
        let rules = rule::Entity::find()
            .filter(rule::Column::ItemType.eq(item_type))
            .all(db!())
            .await?;
        let rules: Vec<i64> = rules.iter().map(|rule| rule.chat_id).collect::<Vec<_>>();
        let message = Response {
            action: format!("获取{}列表", action_str),
            success: true,
            data: format!("{:?}", rules),
        };
        Ok(message)
    }

    /// 添加人
    async fn create_user(
        id: i64,
        group_id: Option<i64>,
        item_type: ItemType,
        action_str: &str,
    ) -> anyhow::Result<Response> {
        // 创建操作的逻辑
        let rule = rule::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            chat_id: sea_orm::ActiveValue::Set(id),
            chat_type: sea_orm::ActiveValue::Set(rule::ChatType::User),
            item_type: sea_orm::ActiveValue::Set(item_type),
            created_at: sea_orm::ActiveValue::Set(chrono::Utc::now().into()),
            updated_at: sea_orm::ActiveValue::Set(chrono::Utc::now().into()),
            group_chat_id: sea_orm::ActiveValue::Set(group_id),
        };
        if rule::Entity::find()
            .filter(rule::Column::ChatId.eq(id))
            .filter(rule::Column::ItemType.eq(item_type))
            .filter(rule::Column::GroupChatId.eq(group_id))
            .one(db!())
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
        rule.save(db!()).await?;
        info!(
            "{:?} add {} of group {} success",
            item_type,
            id,
            group_id.unwrap_or_default()
        );
        let message = Response {
            action: format!("添加{}", action_str),
            success: true,
            data: format!("{}:{}", group_id.unwrap_or_default(), id),
        };
        Ok(message)
    }

    /// 删除人
    async fn delete_user(
        id: i64,
        group_id: Option<i64>,
        item_type: ItemType,
        action_str: &str,
    ) -> anyhow::Result<Response> {
        // 删除操作的逻辑
        let row_affected = rule::Entity::delete_many()
            .filter(rule::Column::ChatId.eq(id))
            .filter(rule::Column::ItemType.eq(item_type))
            .filter(rule::Column::GroupChatId.eq(group_id))
            .exec(db!())
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
        info!(
            "{:?} remove {} of group {} success",
            item_type,
            id,
            group_id.unwrap_or_default()
        );
        let message = Response {
            action: format!("删除{}", action_str),
            success: true,
            data: format!("{}:{}", group_id.unwrap_or_default(), id),
        };
        Ok(message)
    }

    /// 列表人
    async fn list_user(group_id: Option<i64>, item_type: ItemType, action_str: &str) -> anyhow::Result<Response> {
        // 列表操作的逻辑
        let rules = rule::Entity::find()
            .filter(rule::Column::ItemType.eq(item_type))
            .filter(rule::Column::GroupChatId.eq(group_id))
            .all(db!())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_create_group() {
        let command = "黑名单 添加群 123";
        let cli = Cli::parse_command(command);
        println!("{:?}", cli);
    }

    #[test]
    fn test_parse_command_create_user() {
        let command = "黑名单 添加人 123";
        let cli = Cli::parse_command(command);
        println!("{:?}", cli);
    }
}
