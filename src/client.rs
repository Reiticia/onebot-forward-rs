use std::sync::{
    Arc, LazyLock,
    atomic::Ordering,
    atomic::{AtomicBool, AtomicI64},
};

use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::{debug, error, info};
use tokio::{net::TcpStream, sync::RwLock, time};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

use crate::{
    config,
    model::{self, Api},
    server::WsServer,
    utils,
};

pub type Writer = SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>;
pub type Reader = SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>;

#[derive(Debug, Default)]
pub struct WsClient {
    user_id: Option<i64>,
    writer: Option<Writer>,
}

static WS_CLIENT: LazyLock<RwLock<WsClient>> = LazyLock::new(|| RwLock::new(WsClient::default()));
static LAST_HEARTBEAT_TIME: AtomicI64 = AtomicI64::new(0);

impl WsClient {
    /// 创建WS连接
    pub async fn connect() -> anyhow::Result<()> {
        let config = config::APP_CONFIG.clone();
        let url = config.websocket.client_url();
        let is_notice = config.notice.clone();
        info!("try to connect to server");

        tokio::spawn(async move {
            loop {
                let connect_task = connect_async(&url);
                let sleep_task = tokio::time::sleep(time::Duration::from_secs(5));
                let interupt = tokio::signal::ctrl_c();

                tokio::select! {
                    Ok((ws, _resp)) = connect_task => {
                        info!("connect to server {} success", url);
                        if let Err(err) = Self::handle_connect(ws).await {
                            error!("Connection error: {}", err);
                        }
                        // 发送邮件通知消息
                        if let Some(ref notice) = is_notice {
                            utils::send_email(notice.clone(), &WS_CLIENT.read().await.user_id.unwrap_or(0).to_string()).await?;
                        }
                        WS_CLIENT.write().await.writer = None;
                        WS_CLIENT.write().await.user_id = None;
                        info!("server end connection, try to reconnect");
                    },
                    _ = sleep_task => {
                        info!("faile to connect to server, retry in 5 seconds")
                    }
                    _ = interupt => {
                        info!("receive interupt signal, exit");
                        break;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    /// 处理消息
    async fn handle_connect(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> anyhow::Result<()> {
        let (writer, ref mut reader) = ws.split();
        WS_CLIENT.write().await.writer = Some(writer);
        Self::receive(reader).await?;
        Ok(())
    }

    /// 接收消息
    async fn receive(reader: &mut Reader) -> anyhow::Result<()> {
        let active = Arc::new(AtomicBool::new(true));
        while active.load(Ordering::SeqCst) {
            if let Some(msg) = reader.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let str = text.as_str();
                        Self::handle_message(str, active.clone()).await?;
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
        Ok(())
    }

    /// 处理消息
    async fn handle_message(msg: &str, active: Arc<AtomicBool>) -> anyhow::Result<()> {
        if let Ok(event) = serde_json::from_str::<model::Event>(msg) {
            if event.post_type == "meta_event"
                && event.meta_event_type == Some("lifecycle".into())
                && event.sub_type == Some("connect".into())
            {
                // 连接成功事件
                info!("bot connect success");
                // 发送消息通知
                if let Some(user_id) = config::APP_CONFIG.clone().online_notice {
                    let connect_msg = "协议端已连接";
                    let params = format!(
                        r#"{{"user_id": {:?}, "message": [{{"type": "text", "data": {{ "text": {:?} }}}}]}}"#,
                        user_id, connect_msg
                    );
                    let api = Api {
                        action: "send_private_msg".into(),
                        params: serde_json::from_str(&params)?,
                        echo: "1".into(),
                    };
                    WsClient::send(api).await?;
                };

                WS_CLIENT.write().await.user_id = Some(event.self_id);
                LAST_HEARTBEAT_TIME.store(event.time, Ordering::SeqCst);
                // 开启心跳检测事件
                tokio::spawn(Self::heartbeat_active(active.clone()));
            }
            if event.post_type == "meta_event" && event.meta_event_type == Some("heartbeat".into()) {
                LAST_HEARTBEAT_TIME.store(event.time, Ordering::SeqCst);
            }
            WsServer::broadcast_message(event).await?;
        }
        if let Ok(resposne) = serde_json::from_str::<model::ApiResponse>(msg) {
            WsServer::response_message(resposne).await?;
        }
        if let Ok(api) = serde_json::from_str::<model::TestMessage>(msg) {
            let str = serde_json::to_string(&api)?;
            WsServer::broadcast_str_message(&str).await?;
        }
        Ok(())
    }

    /// 发送消息
    pub async fn send(data: Api) -> anyhow::Result<()> {
        let mut ws_client = WS_CLIENT.write().await;

        if let Some(ref mut writer) = ws_client.writer {
            let json = serde_json::to_string(&data)?;
            let message = Message::Text(json.into());
            writer.send(message).await?;
        }
        Ok(())
    }

    /// 检测心跳
    async fn heartbeat_active(active: Arc<AtomicBool>) {
        loop {
            let heartbeat_time = config::APP_CONFIG.websocket.heartbeat;
            let last_heartbeat = LAST_HEARTBEAT_TIME.load(Ordering::SeqCst);
            let duration = time::Instant::now().checked_sub(time::Duration::from_secs(last_heartbeat as u64));
            if duration
                .map(|d| d.elapsed() > time::Duration::from_secs(heartbeat_time as u64))
                .unwrap_or(true)
            {
                // res 为 true 证明心跳超时，需要重连
                active.store(false, Ordering::SeqCst);
                break;
            }

            let sleep_future = tokio::time::sleep(time::Duration::from_secs(heartbeat_time as u64));
            let ctrl_c_future = tokio::signal::ctrl_c();

            tokio::select! {
                _ = sleep_future => (),
                _ = ctrl_c_future => {
                    info!("receive interupt signal, exit");
                    break;
                }
            }
        }
    }

    /// 判断协议端是否存活
    pub async fn alive() -> Option<i64> {
        WS_CLIENT.read().await.user_id
    }
}
