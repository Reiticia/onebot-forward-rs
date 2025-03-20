mod client;
mod config;
mod model;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::APP_CONFIG.init_logger()?;
    client::WsClient::connect().await?;
    server::WsServer::start().await?;
    Ok(())
}
