mod command;
mod model;
mod utils;
mod wss;

use command::BWListCli;
use model::config;

use clap::{Parser, Subcommand};

use crate::utils::send_email_with_template;
use crate::wss::r#impl::ImplSideTrait;

// 顶层命令行结构
#[derive(Parser, Debug)]
pub(crate) struct StartUpCli {
    #[clap(subcommand)]
    command: StartUpCommands, // 子命令：黑名单或白名单
}

#[derive(Subcommand, Debug)]
enum StartUpCommands {
    #[command(visible_alias = "wss")]
    WebSocketServer,
    #[command(visible_alias = "wsc")]
    WebSocketClient,
    #[command(visible_alias = "et")]
    EmailTest {
        #[arg[short]]
        receiver: Option<String>,
        #[arg[short]]
        subject: Option<String>,
        #[arg[short]]
        body: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = StartUpCli::parse();

    config::APP_CONFIG.init_logger()?;
    config::APP_CONFIG.init_db().await?;

    match cli.command {
        StartUpCommands::WebSocketServer => {
            // 启用正向WS模式
            if let Some(websocket) = config::APP_CONFIG.websocket.clone() {
                wss::r#impl::ImplSide::connect(&websocket, config::APP_CONFIG.convert_self).await?;
                wss::sdk::SdkSide::start(&websocket).await?;
            }
        }
        StartUpCommands::WebSocketClient => {
            todo!("启用反向WS模式")
        }
        StartUpCommands::EmailTest {
            receiver,
            subject,
            body,
        } => {
            if let Some(ref email_config) = config::APP_CONFIG.get_notice() {
                let receiver = receiver.unwrap_or(email_config.receiver.clone());
                let subject = subject.unwrap_or("邮件发送测试".into());
                let body = body.unwrap_or("邮件发送测试".into());
                send_email_with_template(email_config, &receiver, &subject, &body).await?;
            }
        }
    }
    Ok(())
}
