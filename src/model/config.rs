use std::{
    path::PathBuf,
    str::FromStr,
    sync::{Arc, LazyLock},
};

use log::info;
use log::{LevelFilter, error};
use migration::{Migrator, MigratorTrait};
use sea_orm::Database;
use tokio::sync::OnceCell;

pub static APP_CONFIG: LazyLock<Arc<AppConfig>> = LazyLock::new(|| {
    let config = init_config().unwrap();
    Arc::new(config)
});

pub static APP_CONFIG_DB: LazyLock<OnceCell<sea_orm::DatabaseConnection>> = LazyLock::new(OnceCell::new);

fn init_config() -> anyhow::Result<AppConfig> {
    let mut config: Option<AppConfig> = None;
    if let Ok(config_str) = std::fs::read_to_string("app.yml") {
        config = Some(serde_yaml::from_str(&config_str)?);
    }
    if let Ok(config_str) = std::fs::read_to_string("app.yaml") {
        config = Some(serde_yaml::from_str(&config_str)?);
    }
    if let Ok(config_str) = std::fs::read_to_string("app.json") {
        config = Some(serde_json::from_str(&config_str)?);
    }
    if let Ok(config_str) = std::fs::read_to_string("app.toml") {
        config = Some(toml::from_str(&config_str)?);
    };
    let config = config.ok_or_else(|| anyhow::anyhow!("配置文件 app.yml/yaml/json/toml 不存在"))?;
    Ok(config)
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub websocket: Option<WebSocketConfig>,
    pub logger: Option<LoggerConfig>,
    pub default_policy: Option<Policy>,
    pub notice: Option<EmailNoticeConfig>,
    pub super_users: Vec<i64>,
    pub config_db_url: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    /// 允许
    Allow,
    /// 拒绝
    #[default]
    Deny,
}

impl AppConfig {
    /// 初始化日志
    pub fn init_logger(&self) -> anyhow::Result<()> {
        let logger = self.logger.clone();
        let exclude = logger.clone().map(|l| l.exclude).unwrap_or_default().unwrap_or(vec![
            "tungstenite::handshake".into(),
            "sqlx::query".into(),
            "sea_orm_migration::migrator".into(),
        ]);
        let console_level = logger.map(|l| l.level).unwrap_or_default();

        let mut dispatch = fern::Dispatch::new()
            // 自定义输出格式
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{}][{}][{}] {}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    record.target(),
                    record.level(),
                    message
                ))
            })
            // 控制台输出配置
            .chain(
                fern::Dispatch::new()
                    // 控制台日志等级
                    .level(console_level.into())
                    .chain(std::io::stdout()),
            );
        for r#mod in exclude {
            dispatch = dispatch
                // 禁用指定模块日志
                .level_for(r#mod, log::LevelFilter::Off)
        }
        if let Some(log_file) = self.logger.clone().unwrap_or_default().file {
            // 判断输出文件位置是否存在，不存在则创建
            let log_file_path = PathBuf::from_str(&log_file.path).unwrap();
            if !log_file_path.parent().unwrap().exists() {
                std::fs::create_dir_all(log_file_path.parent().unwrap())?;
            }

            dispatch = dispatch.chain(
                fern::Dispatch::new()
                    .level(log_file.level.into())
                    .chain(fern::log_file(log_file.path)?),
            )
        }
        dispatch.apply()?;
        Ok(())
    }

    /// 初始化数据库连接
    pub async fn init_db(&self) -> anyhow::Result<sea_orm::DatabaseConnection> {
        let db_url = APP_CONFIG
            .config_db_url
            .clone()
            .unwrap_or("sqlite://forward.sqlite?mode=rwc".into());
        let conn = Database::connect(&db_url).await?;
        Migrator::up(&conn, None).await?;
        info!("Database migration completed");
        if let Err(err) = APP_CONFIG_DB.set(conn.clone()) {
            error!("Failed to set database connection: {:?}", err);
        }
        Ok(conn)
    }

    /// 获取上线通知人
    pub fn get_online_notice_target(&self) -> Option<i64> {
        self.super_users.first().cloned()
    }

    /// 获取邮件通知人
    pub fn get_notice(&self) -> Option<EmailNoticeConfig> {
        self.notice.clone()
    }
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct WebSocketConfig {
    pub server: Server,
    pub client: Server,
    pub heartbeat: i64,
}

impl WebSocketConfig {
    pub fn client_url(&self) -> String {
        format!("ws://{}:{}", self.client.host, self.client.port)
    }
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub secret: Option<String>,
}
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct LoggerConfig {
    pub level: LogLevel,
    pub file: Option<LogFileConfig>,
    pub exclude: Option<Vec<String>>,
}

#[derive(serde::Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
    Off,
}

impl From<LogLevel> for LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Off => LevelFilter::Off,
            LogLevel::Trace => LevelFilter::Trace,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Error => LevelFilter::Error,
        }
    }
}

#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct LogFileConfig {
    pub path: String,
    pub level: LogLevel,
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub struct EmailNoticeConfig {
    pub smtp: String,
    pub username: String,
    pub password: String,
    pub receiver: String,
    pub mail: Option<EmailTemplate>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct EmailTemplate {
    pub subject: String,
    pub body: String,
}
impl Default for EmailTemplate {
    fn default() -> Self {
        Self {
            subject: "你的Bot掉线了".into(),
            body: "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。".into(),
        }
    }
}
