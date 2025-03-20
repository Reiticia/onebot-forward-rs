#![allow(dead_code)]

use std::{
    path::PathBuf,
    str::FromStr,
    sync::{Arc, LazyLock},
};

use log::LevelFilter;

pub static APP_CONFIG: LazyLock<Arc<AppConfig>> = LazyLock::new(|| {
    let config = init_config().unwrap();
    Arc::new(config)
});

fn init_config() -> anyhow::Result<AppConfig> {
    let config_str = std::fs::read_to_string("app.toml")?;
    let config: AppConfig = toml::from_str(&config_str)?;
    Ok(config)
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub websocket: WebSocketConfig,
    pub logger: Option<LoggerConfig>,
    pub blacklist: Option<Vec<i64>>,
    pub whitelist: Option<Vec<i64>>,
    pub notice: Option<EmailNoticeConfig>,
}

impl AppConfig {
    /// 初始化日志
    pub fn init_logger(&self) -> anyhow::Result<()> {
        let console_level = self.logger.clone().map(|l| l.level).unwrap_or_default();

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
}
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct LoggerConfig {
    pub level: LogLevel,
    pub file: Option<LogFileConfig>,
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
    pub port: u16,
    pub username: String,
    pub password: String,
    pub receiver: String,
    pub mail: Option<EmailTemplate>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct EmailTemplate {
    pub title: String,
    pub subject: String,
    pub body: String,
}
impl Default for EmailTemplate {
    fn default() -> Self {
        Self {
            title: "你的Bot掉线了".into(),
            subject: "OneBot 掉线通知".into(),
            body: "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。".into(),
        }
    }
}
