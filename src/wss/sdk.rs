use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, LazyLock},
};

use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::{error, info, trace};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::RwLock,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::{
    config::WebSocketConfig,
    model::onebot::{Api, ApiResponse, ConnectMessage, Event},
    utils,
    wss::r#impl::ImplSide,
};

pub type Writer = SplitSink<WebSocketStream<TcpStream>, Message>;
pub type Reader = SplitStream<WebSocketStream<TcpStream>>;

static SDK_SIDE: LazyLock<RwLock<SdkSide>> = LazyLock::new(|| RwLock::new(SdkSide::default()));

#[derive(Debug, Default)]
pub struct SdkSide {
    pub writers_map: HashMap<String, Arc<RwLock<Writer>>>,
    pub echo_map: HashMap<String, Arc<RwLock<Writer>>>,
}

impl SdkSide {
    /// 创建WS服务
    pub async fn start(websocket: &WebSocketConfig) -> anyhow::Result<()> {
        let host = websocket.server.host.clone();
        let port = websocket.server.port;
        let addr = format!("{}:{}", &host, &port);
        // Create the event loop and TCP listener we'll accept connections on.
        let try_socket = TcpListener::bind(&addr).await;
        let listener = try_socket.expect("Failed to bind");
        info!("Listening on: {}", addr);

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
        SDK_SIDE
            .write()
            .await
            .writers_map
            .insert(addr.to_string(), writer.clone());

        tokio::spawn(async move { Self::handle_connect(&mut incoming, writer).await });
    }

    /// 处理连接
    async fn handle_connect(reader: &mut Reader, writer: Arc<RwLock<Writer>>) -> anyhow::Result<()> {
        // 判断协议端是否以及连接，如果协议端已连接，则推送连接成功消息给客户端
        if let Some(user_id) = ImplSide::alive().await {
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
                            trace!("receive ping message");
                        }
                        Ok(Message::Pong(_)) => {
                            trace!("receive pong message");
                        }
                        Ok(Message::Close(_)) => {
                            info!("receive close message");
                            break;
                        }
                        Ok(_) => {
                            info!("receive non-text message");
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
        SDK_SIDE.write().await.writers_map.retain(|k, w| {
            if Arc::ptr_eq(w, &writer) {
                info!("remove client: {}", k);
                false
            } else {
                true
            }
        });
        Ok(())
    }

    /// 处理消息
    async fn handle_message(msg: &str, writer: Arc<RwLock<Writer>>) -> anyhow::Result<()> {
        let api = serde_json::from_str::<Api>(msg)?;
        let echo = &api.echo;
        let mut ws_server = SDK_SIDE.write().await;

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
        ImplSide::send(api).await?;
        Ok(())
    }

    /// 将消息广播给所有客户端
    pub async fn broadcast_message(msg: Event) -> anyhow::Result<()> {
        trace!("broadcast event: {:?}", msg);
        let msg = serde_json::to_string(&msg)?;
        let msg = Message::Text(msg.into());
        for (_, writer) in SDK_SIDE.read().await.writers_map.clone() {
            writer.write().await.send(msg.clone()).await?;
        }
        Ok(())
    }

    /// 将API调用返回给调用方
    pub async fn response_message(msg: ApiResponse) -> anyhow::Result<()> {
        trace!("feedback response: {:?}", msg);
        let echo = &msg.echo;
        let mut ws_server = SDK_SIDE.write().await;
        if let Some(writer) = ws_server.echo_map.remove(echo) {
            writer
                .write()
                .await
                .send(Message::Text(serde_json::to_string(&msg)?.into()))
                .await?;
        }
        Ok(())
    }
}
