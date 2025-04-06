mod command;
mod model;
mod utils;
mod wss;

use command::Cli;
use model::config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::APP_CONFIG.init_logger()?;
    config::APP_CONFIG.init_db().await?;
    // 启用正向WS模式
    if let Some(websocket) = config::APP_CONFIG.websocket.clone() {
        wss::r#impl::ImplSide::connect(&websocket).await?;
        wss::sdk::SdkSide::start(&websocket).await?;
    }
    // TODO 启用反向WS模式

    Ok(())
}
