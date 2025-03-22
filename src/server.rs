use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, LazyLock},
};

use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::{debug, error, info};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::RwLock,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::{
    client::WsClient,
    config,
    model::{self, ApiResponse, ConnectMessage, Event},
    utils,
};

pub type Writer = SplitSink<WebSocketStream<TcpStream>, Message>;
pub type Reader = SplitStream<WebSocketStream<TcpStream>>;

static WS_SERVER: LazyLock<RwLock<WsServer>> = LazyLock::new(|| RwLock::new(WsServer::default()));

#[derive(Debug, Default)]
pub struct WsServer {
    pub writers: Vec<Arc<RwLock<Writer>>>,
    pub echo_map: HashMap<String, Arc<RwLock<Writer>>>,
}

impl WsServer {
    /// 创建WS服务
    pub async fn start() -> anyhow::Result<()> {
        let websocket = config::APP_CONFIG.websocket.clone();
        let host = websocket.server.host;
        let port = websocket.server.port;
        let addr = format!("{}:{}", &host, &port);
        // Create the event loop and TCP listener we'll accept connections on.
        let try_socket = TcpListener::bind(&addr).await;
        let listener = try_socket.expect("Failed to bind");
        println!("Listening on: {}", addr);

        // 接收客户端连接
        loop {
            tokio::select! {
                Ok((stream, addr)) = listener.accept() => {
                    tokio::spawn(Self::handle_connection(stream, addr));
                },
                _ = utils::ctrl_c_signal() => {
                    break;
                }
            }
        }

        Ok(())
    }

    pub async fn handle_connection(raw_stream: TcpStream, addr: SocketAddr) {
        info!("New client connection from: {}", addr);

        let ws_stream = tokio_tungstenite::accept_async(raw_stream)
            .await
            .expect("Error during the websocket handshake occurred");
        info!("WebSocket connection established: {}", addr);

        let (outgoing, mut incoming) = ws_stream.split();
        let writer = Arc::new(RwLock::new(outgoing));
        WS_SERVER.write().await.writers.push(writer.clone());

        tokio::spawn(async move { Self::handle_connect(&mut incoming, writer).await });
    }

    /// 处理连接
    async fn handle_connect(reader: &mut Reader, writer: Arc<RwLock<Writer>>) -> anyhow::Result<()> {
        // 判断协议端是否以及连接，如果协议端已连接，则推送连接成功消息给客户端
        if let Some(user_id) = WsClient::alive().await {
            let msg = ConnectMessage::new(user_id);
            let msg = serde_json::to_string(&msg)?;
            let message = Message::Text(msg.into());
            writer.write().await.send(message).await?;
            info!("send connect lifecycle message to client");
        }

        loop {
            tokio::select! {
                Some(msg) =  reader.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            let str = text.as_str();
                            let writer = writer.clone();
                            Self::handle_message(str, writer).await?;
                        }
                        Ok(Message::Ping(_)) => {
                            debug!("receive ping message");
                        }
                        Ok(Message::Pong(_)) => {
                            debug!("receive pong message");
                        }
                        Ok(_) => {
                            debug!("receive non-text message");
                            break;
                        }
                        Err(err) => {
                            error!("receive error: {}", err);
                            break;
                        }
                    }
                },
                _ = utils::ctrl_c_signal() => {
                    info!("receive ctrl-c signal");
                    break;
                }
            }
        }
        // 删除对应客户端
        WS_SERVER.write().await.writers.retain(|w| !Arc::ptr_eq(w, &writer));
        Ok(())
    }

    /// 处理消息
    async fn handle_message(msg: &str, writer: Arc<RwLock<Writer>>) -> anyhow::Result<()> {
        let api = serde_json::from_str::<model::Api>(msg)?;
        let echo = &api.echo;
        let mut ws_server = WS_SERVER.write().await;

        // 黑白名单过滤
        if let Some(Some(group_id)) = api.params.get("group_id").map(|v| v.as_i64()) {
            if !utils::send_by_auth(group_id) {
                return Ok(());
            }
        }

        ws_server
            .echo_map
            .insert(echo.clone().unwrap_or_default(), writer.clone());
        // 将消息发送给对应 OneBot 协议端
        WsClient::send(api).await?;
        Ok(())
    }

    /// 将消息广播给所有客户端
    pub async fn broadcast_message(msg: Event) -> anyhow::Result<()> {
        let msg = serde_json::to_string(&msg)?;
        let msg = Message::Text(msg.into());
        for writer in WS_SERVER.read().await.writers.clone() {
            writer.write().await.send(msg.clone()).await?;
        }
        Ok(())
    }

    /// 将API调用返回给调用方
    pub async fn response_message(msg: ApiResponse) -> anyhow::Result<()> {
        let echo = &msg.echo;
        let mut ws_server = WS_SERVER.write().await;
        if let Some(writer) = ws_server.echo_map.remove(echo) {
            writer
                .write()
                .await
                .send(Message::Text(serde_json::to_string(&msg)?.into()))
                .await?;
        }
        Ok(())
    }

    /// 将字符串消息广播给所有客户端
    pub async fn broadcast_str_message(msg: &str) -> anyhow::Result<()> {
        let msg = Message::Text(msg.into());
        for writer in WS_SERVER.read().await.writers.clone() {
            writer.write().await.send(msg.clone()).await?;
        }
        Ok(())
    }
}
