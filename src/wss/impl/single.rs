use super::{Reader, Writer};
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering},
    },
    time::{self, Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, stream::StreamExt};
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
    model::{
        app::InteruptReason,
        onebot::{Api, ApiResponse, Event},
    },
    utils::{DatabaseCache, generate_random_string, send_email},
    wss::{r#impl::ImplSideTrait, sdk::SdkSide},
};

type EchoTxMap = RwLock<HashMap<String, mpsc::Sender<ApiResponse>>>;

#[derive(Debug, Default)]
pub struct SingleImplSide {
    /// 登录账号ID
    user_id: RwLock<Option<i64>>,
    /// WebSocket写入器
    writer: RwLock<Option<Writer>>,
    /// 上次心跳时间，单位秒
    last_heartbeat: AtomicI64,
    /// 是否转换自身消息
    convert_self: OnceLock<bool>,
    /// 心跳间隔时间，单位秒
    heartbeat_interval: OnceLock<i64>,
    /// 每一个echo标识对应不同的响应，用于减少重复的协议端API调用
    echo_tx_map: EchoTxMap,
    /// 是否为连接断开，用于决定在连接断开后重连时是否发送邮件
    disconnect: AtomicBool,
}

static SINGLE_IMPL_SIDE: LazyLock<SingleImplSide> = LazyLock::new(|| SingleImplSide::default());

impl ImplSideTrait for SingleImplSide {
    async fn connect(websocket: &WebSocketConfig, convert_self: Option<bool>) -> anyhow::Result<()> {
        info!("start websocket client mode");
        SINGLE_IMPL_SIDE.convert_self.set(convert_self.unwrap_or_default()).ok();
        SINGLE_IMPL_SIDE.heartbeat_interval.set(websocket.heartbeat).ok();
        let is_notice = config::APP_CONFIG.get_notice();
        info!("try to connect to server");

        let index = AtomicU32::new(0);
        let websocket = websocket.clone();
        tokio::spawn(async move {
            loop {
                let (url, secret) = websocket.client_url(index.load(Ordering::SeqCst));
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
                        SINGLE_IMPL_SIDE.disconnect.store(false, Ordering::SeqCst);
                        match Self::handle_connect(ws).await {
                            Ok(signal) => {
                                if matches!(signal, InteruptReason::ShutDown) {
                                    info!("receive signal: {:?}, exit connect", signal);
                                    break;
                                }
                            },
                            Err(err) => error!("Connection error: {}", err),
                        }
                        // 连接中断
                        if let Some(ref notice) = is_notice
                            && SINGLE_IMPL_SIDE.disconnect.load(Ordering::SeqCst)
                                && let Err(err) = send_email(notice.clone(), &SINGLE_IMPL_SIDE.user_id.read().await.unwrap_or(0).to_string()).await {
                                    error!("email send fail: {:?}", err);
                                }
                        *SINGLE_IMPL_SIDE.writer.write().await = None;
                        *SINGLE_IMPL_SIDE.user_id.write().await = None;
                        index.fetch_add(1, Ordering::SeqCst);
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
            anyhow::Ok(())
        });
        Ok(())
    }
    async fn alive() -> Option<i64> {
        *SINGLE_IMPL_SIDE.user_id.read().await
    }
    async fn send(data: Api) -> anyhow::Result<()> {
        trace!("invoke api: {:?}", data);

        if let Some(ref mut writer) = *SINGLE_IMPL_SIDE.writer.write().await {
            let json = serde_json::to_string(&data)?;
            let message = Message::Text(json.into());
            writer.send(message).await?;
            debug!("send message success")
        }
        Ok(())
    }
}

impl SingleImplSide {
    /// 处理消息
    async fn handle_connect(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> anyhow::Result<InteruptReason> {
        let (writer, ref mut reader) = ws.split();
        *SINGLE_IMPL_SIDE.writer.write().await = Some(writer);
        Self::receive(reader).await
    }

    /// 接收消息
    async fn receive(reader: &mut Reader) -> anyhow::Result<InteruptReason> {
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
            Ok(InteruptReason::ShutDown)
        } else {
            Ok(InteruptReason::ReConnect)
        }
    }

    /// 处理消息
    async fn handle_message(msg: &str, active: Arc<AtomicBool>) -> anyhow::Result<()> {
        if let Ok(ref mut event) = serde_json::from_str::<Event>(msg) {
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
                    Self::send(api).await?;
                };

                *SINGLE_IMPL_SIDE.user_id.write().await = Some(event.self_id);
                SINGLE_IMPL_SIDE.last_heartbeat.store(event.time, Ordering::SeqCst);
                // 开启心跳检测事件
                tokio::spawn(Self::heartbeat_active(active.clone()));
                // 开启定时任务检查账号状态
                tokio::spawn(Self::active_check(active.clone()));
            }
            if event.post_type == "meta_event" && event.meta_event_type == Some("heartbeat".into()) {
                SINGLE_IMPL_SIDE.last_heartbeat.store(event.time, Ordering::SeqCst);
                debug!("receive heartbeat message");
                return Ok(());
            }
            // 判断超级管理员，若是，尝试解析指令
            if event.post_type == "message"
                && let Some(user_id) = event.user_id
            {
                // 如果是超级管理员，则匹配请求命令
                if config::APP_CONFIG.super_users.contains(&user_id)
                    && let Ok(command) =
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

            // 判断黑白名单配置
            if !DatabaseCache::send_by_auth(event.group_id, event.user_id).await {
                return Ok(());
            }
            // 此处判断是否为自身消息且是否需要转换自身消息
            if let Some(enable) = SINGLE_IMPL_SIDE.convert_self.get()
                && *enable
                && event.post_type == "message_sent"
            {
                event.post_type = "message".into();
            }

            SdkSide::broadcast_message(event).await?;
        }
        if let Ok(resposne) = serde_json::from_str::<ApiResponse>(msg) {
            // 判断是否由内部发出Api的响应消息
            if let Some(echo) = resposne.echo.clone() {
                if let Some(tx) = SINGLE_IMPL_SIDE.echo_tx_map.read().await.get(&echo) {
                    info!("receive internal api response");
                    tx.send(resposne).await?;
                } else {
                    SdkSide::response_message(resposne).await?;
                }
            }
        }
        Ok(())
    }

    /// 检测心跳
    async fn heartbeat_active(active: Arc<AtomicBool>) {
        info!("start heartbeat active task");
        let heartbeat_time = *SINGLE_IMPL_SIDE.heartbeat_interval.get().unwrap();
        loop {
            let last_heartbeat = SINGLE_IMPL_SIDE.last_heartbeat.load(Ordering::SeqCst);
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
                        SINGLE_IMPL_SIDE.user_id.read().await.unwrap_or(0)
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

    /// 发送存活请求到协议端
    async fn send_active_check_message(active: Arc<AtomicBool>) {
        let echo = generate_random_string(4);
        let api = Api {
            action: "get_status".into(),
            params: HashMap::new(),
            echo: Some(echo.clone()),
        };
        let (tx, mut rx) = mpsc::channel::<ApiResponse>(1);
        let mut echo_tx_map_writer = SINGLE_IMPL_SIDE.echo_tx_map.write().await;
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
                } else {
                    // 如果断开连接状态位为 false 则将其设置为 true，以便在连接中断时发送邮件提醒
                    if !SINGLE_IMPL_SIDE.disconnect.load(Ordering::SeqCst) {
                        SINGLE_IMPL_SIDE.disconnect.store(true, Ordering::SeqCst);
                    }
                }
            }
        });
        // 如果请求发送失败则移除该通道
        if Self::send(api).await.is_err() {
            error!("send get_status api failed");
            echo_tx_map_writer.remove(&echo);
        } else {
            info!("send get_status api success");
        }
    }
}
