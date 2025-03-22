#![allow(dead_code)]

use std::{
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::{self, Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::{debug, error, info};
use tokio::{net::TcpStream, sync::RwLock};
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

                tokio::select! {
                    Ok((ws, _resp)) = connect_task => {
                        info!("connect to server {} success", url);
                        match Self::handle_connect(ws).await {
                            Ok(signal) => {
                                if signal != 0 {
                                    info!("receive signal: {}, exit connect", signal);
                                    break;
                                }
                            },
                            Err(err) => error!("Connection error: {}", err),
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
                    _ = utils::ctrl_c_signal() => {
                        info!("receive interupt signal, exit connect");
                        break;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
        Ok(())
    }

    /// 处理消息
    async fn handle_connect(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> anyhow::Result<i64> {
        let (writer, ref mut reader) = ws.split();
        WS_CLIENT.write().await.writer = Some(writer);
        Self::receive(reader).await
    }

    /// 接收消息
    async fn receive(reader: &mut Reader) -> anyhow::Result<i64> {
        let active = Arc::new(AtomicBool::new(true));
        let interupt_flag = AtomicBool::new(false);
        while active.load(Ordering::SeqCst) {
            tokio::select! {
                Some(msg) = reader.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            let str = text.as_str();
                            Self::handle_message(str, active.clone()).await?;
                        }
                        Ok(Message::Ping(_)) => {
                            debug!("receive ping message");
                        }
                        Ok(Message::Pong(_)) => {
                            debug!("receive pong message");
                        }
                        Ok(_) => {
                            debug!("receive non-text message");
                            // break;
                        }
                        Err(err) => {
                            error!("receive error: {}", err);
                            break;
                        }
                    }
                },
                _ = utils::ctrl_c_signal() => {
                    info!("receive interupt signal, exit receive");
                    interupt_flag.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        if interupt_flag.load(Ordering::SeqCst) {
            Ok(-1)
        } else {
            Ok(0)
        }
    }

    /// 处理消息
    async fn handle_message(msg: &str, _active: Arc<AtomicBool>) -> anyhow::Result<()> {
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
                        echo: None,
                    };
                    info!("send connect message to user_id: {}", user_id);
                    WsClient::send(api).await?;
                };

                WS_CLIENT.write().await.user_id = Some(event.self_id);
                LAST_HEARTBEAT_TIME.store(event.time, Ordering::SeqCst);
                // 开启心跳检测事件
                // tokio::spawn(Self::heartbeat_active(active.clone()));
            }
            if event.post_type == "meta_event" && event.meta_event_type == Some("heartbeat".into()) {
                LAST_HEARTBEAT_TIME.store(event.time, Ordering::SeqCst);
            }

            // 黑白名单配置
            if let Some(group_id) = event.group_id {
                if !utils::send_by_auth(group_id) {
                    return Ok(());
                }
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
            let last_heartbeat = UNIX_EPOCH + Duration::from_secs(last_heartbeat as u64);
            match SystemTime::now().duration_since(last_heartbeat) {
                Ok(duration) if duration > Duration::from_secs(heartbeat_time as u64) => {
                    // 触发重连逻辑
                    // res 为 true 证明心跳超时，需要重连
                    info!(
                        "last_heartbeat: {:?}, heartbeat_time: {}, duration: {:?}, try to reconnect",
                        last_heartbeat, heartbeat_time, duration
                    );
                    active.store(false, Ordering::SeqCst);
                    info!(
                        "{} heartbeat timeout, try to reconnect",
                        WS_CLIENT.read().await.user_id.unwrap_or(0)
                    );
                    break;
                }
                Err(_) => {
                    // 时间倒流时的处理
                    error!("System time earlier than last heartbeat");
                }
                _ => continue,
            }

            let sleep_future = tokio::time::sleep(time::Duration::from_secs(heartbeat_time as u64));

            tokio::select! {
                _ = sleep_future => (),
                _ = utils::ctrl_c_signal() => {
                    info!("receive interupt signal, exit heartbeat_active");
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
