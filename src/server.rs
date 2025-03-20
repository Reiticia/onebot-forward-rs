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
    model::{self, Api, ApiResponse, Event},
};

pub type Writer = SplitSink<WebSocketStream<TcpStream>, Message>;
pub type Reader = SplitStream<WebSocketStream<TcpStream>>;

static WS_SERVER: LazyLock<RwLock<WsServer>> = LazyLock::new(|| RwLock::new(WsServer::default()));

#[derive(Debug, Default)]
pub struct WsServer {
    pub writers: Vec<Arc<RwLock<Writer>>>,
    pub echo_map: HashMap<String, Arc<RwLock<Writer>>>,
    pub api_map: HashMap<String, Api>,
    pub same_echo_map: HashMap<String, Vec<String>>,
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
        while let Ok((stream, addr)) = listener.accept().await {
            tokio::spawn(Self::handle_connection(stream, addr));
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
        loop {
            if let Some(msg) = reader.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let str = text.as_str();
                        let writer = writer.clone();
                        Self::handle_message(str, writer).await?;
                    }
                    Ok(Message::Ping(_) | Message::Pong(_)) => {
                        debug!("receive ping/pong message");
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

        let api_map_clone = ws_server.api_map.clone();
        {
            // 鉴于一个 api 会请求多次结果，只进行第一次请求，后续相同请求收入 echo_request_map
            let mut this_api = api.clone();
            let this_echo = this_api.echo.clone();
            this_api.echo = String::new();
            let mut denplicated_request = false;
            for (echo, api_in_map) in api_map_clone.iter() {
                if matches!(api_in_map, _this_api) {
                    ws_server
                        .same_echo_map
                        .entry(echo.clone())
                        .or_insert_with(Vec::new)
                        .push(this_echo.clone());
                    denplicated_request = true;
                    break;
                }
            }
            if !denplicated_request {
                ws_server.api_map.insert(this_echo.clone(), this_api);
            }
        }

        ws_server.echo_map.insert(echo.clone(), writer.clone());
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
        let writer = ws_server
            .echo_map
            .remove(echo)
            .ok_or_else(|| anyhow::anyhow!("echo not found"))?;
        let resps: Vec<ApiResponse> = ws_server
            .same_echo_map
            .remove(echo)
            .ok_or_else(|| anyhow::anyhow!("echo not found"))?
            .iter()
            .map(|echo| {
                let mut msg_clone = msg.clone();
                msg_clone.echo = echo.clone();
                msg_clone
            })
            .collect();
        for resp in resps {
            writer
                .write()
                .await
                .send(Message::Text(serde_json::to_string(&resp)?.into()))
                .await?;
        }
        Ok(())
    }
}
