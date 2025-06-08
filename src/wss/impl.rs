use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::{self, Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::{debug, error, info, trace, warn};
use tokio::{
    net::TcpStream,
    sync::{RwLock, mpsc},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

use crate::{
    BWListCli,
    config::{self, WebSocketConfig},
    ctrl_c_signal,
    model::onebot::{Api, ApiResponse, Event},
    utils::{DatabaseCache, generate_random_string, send_email},
    wss::sdk::SdkSide,
};

pub type Writer = SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>;
pub type Reader = SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>;
type EchoTxMap = Arc<RwLock<HashMap<String, mpsc::Sender<ApiResponse>>>>;

#[derive(Debug, Default)]
pub struct ImplSide {
    user_id: Option<i64>,
    writer: Option<Writer>,
}

static IMPL_SIDE: LazyLock<RwLock<ImplSide>> = LazyLock::new(|| RwLock::new(ImplSide::default()));
static LAST_HEARTBEAT_TIME: AtomicI64 = AtomicI64::new(0);
static HEARTBEAT_INTERVAL: OnceLock<i64> = OnceLock::new();
static ECHO_TX_MAP: LazyLock<EchoTxMap> = LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

impl ImplSide {
    /// 创建WS连接
    pub async fn connect(websocket: &WebSocketConfig) -> anyhow::Result<()> {
        info!("start websocket client mode");
        let url = websocket.client_url();
        let secret = websocket.client.secret.clone();
        HEARTBEAT_INTERVAL.get_or_init(|| websocket.heartbeat);
        let is_notice = config::APP_CONFIG.get_notice();
        info!("try to connect to server");

        tokio::spawn(async move {
            loop {
                let mut request = url.as_str().into_client_request().unwrap();
                if let Some(ref secret) = secret {
                    let access_token = format!("Bearer {}", secret);
                    request
                        .headers_mut()
                        .insert("Authorization", access_token.parse().unwrap());
                }
                let connect_task = connect_async(request);
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
                        if let Some(ref notice) = is_notice {
                            if let Err(err) = send_email(notice.clone(), &IMPL_SIDE.read().await.user_id.unwrap_or(0).to_string()).await {
                                error!("email send fail: {:?}", err);
                            }
                        }
                        IMPL_SIDE.write().await.writer = None;
                        IMPL_SIDE.write().await.user_id = None;
                        info!("server end connection, try to reconnect");
                    },
                    _ = sleep_task => {
                        warn!("faile to connect to server, retry in 5 seconds")
                    }
                    _ = ctrl_c_signal!() => {
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
        IMPL_SIDE.write().await.writer = Some(writer);
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
                _ = ctrl_c_signal!() => {
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
    async fn handle_message(msg: &str, active: Arc<AtomicBool>) -> anyhow::Result<()> {
        if let Ok(event) = serde_json::from_str::<Event>(msg) {
            if event.post_type == "meta_event"
                && event.meta_event_type == Some("lifecycle".into())
                && event.sub_type == Some("connect".into())
            {
                // 连接成功事件
                info!("bot connect success");
                // 发送消息通知
                if let Some(user_id) = config::APP_CONFIG.get_online_notice_target() {
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
                    ImplSide::send(api).await?;
                };

                IMPL_SIDE.write().await.user_id = Some(event.self_id);
                LAST_HEARTBEAT_TIME.store(event.time, Ordering::SeqCst);
                // 开启心跳检测事件
                tokio::spawn(Self::heartbeat_active(active.clone()));
                // 开启定时任务检查账号状态
                tokio::spawn(Self::active_check(active.clone()));
            }
            if event.post_type == "meta_event" && event.meta_event_type == Some("heartbeat".into()) {
                LAST_HEARTBEAT_TIME.store(event.time, Ordering::SeqCst);
                debug!("receive heartbeat message");
                return Ok(());
            }
            // 判断超级管理员，若是，尝试解析指令
            if event.post_type == "message" {
                if let Some(user_id) = event.user_id {
                    // 如果是超级管理员，则匹配请求命令
                    if config::APP_CONFIG.super_users.contains(&user_id) {
                        if let Ok(command) =
                            BWListCli::parse_command(event.raw_message.clone().unwrap_or_default().as_str())
                        {
                            let response = command.execute().await?;
                            let resp_str = format!(
                                "{}操作{}\n结果：{}",
                                response.action,
                                if response.success { "成功" } else { "失败" },
                                response.data
                            );
                            let api = if let Some(group_id) = event.group_id {
                                Api {
                                    action: "send_group_msg".into(),
                                    params: serde_json::from_str(&format!(
                                        r#"{{"group_id": {}, "message": [{{"type": "text", "data": {{ "text": {:?} }}}}]}}"#,
                                        group_id, resp_str
                                    ))?,
                                    echo: None,
                                }
                            } else {
                                Api {
                                    action: "send_private_msg".into(),
                                    params: serde_json::from_str(&format!(
                                        r#"{{"user_id": {}, "message": [{{"type": "text", "data": {{ "text": {:?} }}}}]}}"#,
                                        user_id, resp_str
                                    ))?,
                                    echo: None,
                                }
                            };
                            Self::send(api).await?;
                        }
                    }
                }
            }

            // 判断黑白名单配置
            if !DatabaseCache::send_by_auth(event.group_id, event.user_id).await {
                return Ok(());
            }
            SdkSide::broadcast_message(event).await?;
        }
        if let Ok(resposne) = serde_json::from_str::<ApiResponse>(msg) {
            // 判断是否由内部发出Api的响应消息
            if let Some(echo) = resposne.echo.clone() {
                if let Some(tx) = ECHO_TX_MAP.read().await.get(&echo) {
                    info!("receive internal api response");
                    tx.send(resposne).await?;
                } else {
                    SdkSide::response_message(resposne).await?;
                }
            }
        }
        Ok(())
    }

    /// 发送消息
    pub async fn send(data: Api) -> anyhow::Result<()> {
        trace!("invoke api: {:?}", data);
        let mut ws_client = IMPL_SIDE.write().await;

        if let Some(ref mut writer) = ws_client.writer {
            let json = serde_json::to_string(&data)?;
            let message = Message::Text(json.into());
            writer.send(message).await?;
            debug!("send message success")
        }
        Ok(())
    }

    /// 检测心跳
    async fn heartbeat_active(active: Arc<AtomicBool>) {
        info!("start heartbeat active task");
        loop {
            let heartbeat_time = *HEARTBEAT_INTERVAL.get().unwrap();
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
                    Self::send_active_check_message(active.clone()).await;
                    info!(
                        "{} heartbeat timeout, try to reconnect",
                        IMPL_SIDE.read().await.user_id.unwrap_or(0)
                    );
                    break;
                }
                Err(_) => {
                    // 时间倒流时的处理
                    error!("System time earlier than last heartbeat");
                }
                _ => (),
            }

            let sleep_future = tokio::time::sleep(time::Duration::from_secs(heartbeat_time as u64));

            tokio::select! {
                _ = sleep_future => (),
                _ = ctrl_c_signal!() => {
                    info!("receive interupt signal, exit heartbeat_active");
                    break;
                }
            }
        }
        info!("heartbeat active task exit")
    }

    /// 检查登录号状态是否存活
    async fn active_check(active: Arc<AtomicBool>) {
        info!("start active check task");

        let mut interval = tokio::time::interval(time::Duration::from_secs(3600)); // 每小时触发一次

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    info!("check account login state");
                    Self::send_active_check_message(active.clone()).await;
                }
                _ = ctrl_c_signal!() => {
                    info!("receive interupt signal, exit active_check");
                    break;
                }
                _ = async { while active.load(Ordering::SeqCst) { tokio::time::sleep(time::Duration::from_millis(100)).await; } } => {
                    info!("active flag set to false, exit active_check");
                    break;
                }
            }
        }
        info!("active check task exit");
    }

    /// 判断协议端是否存活
    pub async fn alive() -> Option<i64> {
        IMPL_SIDE.read().await.user_id
    }

    /// 发送存活请求到协议端
    async fn send_active_check_message(active: Arc<AtomicBool>) {
        let echo = generate_random_string(4);
        let api = Api {
            action: "get_status".into(),
            params: HashMap::new(),
            echo: Some(echo.clone()),
        };
        let (tx, mut rx) = mpsc::channel::<ApiResponse>(1);
        let mut echo_tx_map_writer = ECHO_TX_MAP.write().await;
        echo_tx_map_writer.insert(echo.clone(), tx);
        tokio::spawn(async move {
            if let Some(api_response) = rx.recv().await {
                info!("receive get_status api response");
                let data = api_response.data;
                let online = data
                    .get("online")
                    .map(|online| online.as_bool().unwrap_or_default())
                    .unwrap_or_default();
                let good = data
                    .get("good")
                    .map(|online| online.as_bool().unwrap_or_default())
                    .unwrap_or_default();
                if !online || !good {
                    // 判断为登录账号已下线
                    active.store(false, Ordering::SeqCst);
                }
            }
        });
        // 如果请求发送失败则移除该通道
        if ImplSide::send(api).await.is_err() {
            error!("send get_status api failed");
            echo_tx_map_writer.remove(&echo);
        } else {
            info!("send get_status api success");
        }
    }
}
