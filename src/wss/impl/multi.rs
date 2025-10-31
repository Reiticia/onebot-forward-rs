use super::{Reader, Writer};
use crate::BWListCli;
use crate::ctrl_c_signal;
use crate::model::app::InteruptReason;
use crate::utils::DatabaseCache;
use crate::utils::generate_random_string;
use crate::wss::r#impl::ImplSideTrait;
use crate::wss::sdk::SdkSide;
use crate::{
    config::{self, WebSocketConfig},
    model::onebot::{Api, ApiResponse, Event},
};
use futures_util::{SinkExt, stream::StreamExt};
use log::{debug, error, info, trace, warn};
use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, OnceLock, atomic::AtomicBool},
    time::{self, Duration, SystemTime},
};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[derive(Debug, Default)]
pub struct MultiImplSide {
    /// 记录发送器 id -> Writer
    writers: RwLock<HashMap<i64, Arc<RwLock<Writer>>>>,
    /// 记录发送器 群组_id/成员_id -> Writer
    resp_writers: RwLock<HashMap<String, Arc<RwLock<Writer>>>>,
    /// 记录最后心跳时间 id -> 时间戳
    last_heartbeat_time: RwLock<HashMap<i64, i64>>,
    /// 是否转换自身消息
    convert_self: OnceLock<bool>,
    /// 心跳间隔
    heartbeat_interval: OnceLock<i64>,
    /// 是否断开连接 id -> bool
    disconnect: RwLock<HashMap<i64, bool>>,
}

static MULTI_IMPL_SIDE: LazyLock<MultiImplSide> = LazyLock::new(|| MultiImplSide::default());

impl ImplSideTrait for MultiImplSide {
    async fn connect(websocket: &WebSocketConfig, convert_self: Option<bool>) -> anyhow::Result<()> {
        info!("start websocket multi client mode");
        MULTI_IMPL_SIDE.convert_self.set(convert_self.unwrap_or_default()).ok();
        MULTI_IMPL_SIDE.heartbeat_interval.set(websocket.heartbeat).ok();
        let is_notice = config::APP_CONFIG.get_notice();
        info!("try to connect to servers");
        for (url, secret) in websocket.client_urls() {
            let mut request = url.as_str().into_client_request().unwrap();
            if let Some(ref secret) = secret {
                let access_token = format!("Bearer {}", secret);
                request
                    .headers_mut()
                    .insert("Authorization", access_token.parse().unwrap());
            }
            let Ok((ws, _response)) = connect_async(request).await else {
                warn!("faile to connect to server {}, skip", url);
                continue;
            };
            tokio::spawn(Self::handle_connect(ws));
        }

        Ok(())
    }

    /// 判断协议端是否存活
    async fn alive() -> Vec<Option<i64>> {
        MULTI_IMPL_SIDE
            .disconnect
            .read()
            .await
            .iter()
            .filter(|(_, v)| **v)
            .map(|(k, _)| Some(*k))
            .collect()
    }

    /// 发送消息
    async fn send(_data: Api) -> anyhow::Result<()> {
        unimplemented!()
    }
}

impl MultiImplSide {
    /// 处理消息
    async fn handle_connect(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> anyhow::Result<InteruptReason> {
        let (writer, ref mut reader) = ws.split();

        let interupt_flag = AtomicBool::new(false);

        let writer = Arc::new(RwLock::new(writer));

        while active.load(Ordering::SeqCst) {
            tokio::select! {
                Some(msg) = reader.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            let str = text.as_str();
                            Self::handle_message(str, writer.clone()).await?;
                        }
                        Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                            trace!("receive ping/pong message");
                        }
                        Ok(_) | Err(_) => {
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

    async fn handle_message(msg: &str, writer: Arc<RwLock<Writer>>) -> anyhow::Result<()> {
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

                MULTI_IMPL_SIDE
                    .writers
                    .write()
                    .await
                    .insert(event.self_id, writer.clone());
                MULTI_IMPL_SIDE
                    .last_heartbeat_time
                    .write()
                    .await
                    .insert(event.self_id, event.time);
                MULTI_IMPL_SIDE.disconnect.write().await.insert(event.self_id, false);
                // 开启心跳检测事件
                tokio::spawn(Self::heartbeat_active(event.self_id));
                // 开启定时任务检查账号状态
                tokio::spawn(Self::active_check(event.self_id));
            }
            if event.post_type == "meta_event" && event.meta_event_type == Some("heartbeat".into()) {
                MULTI_IMPL_SIDE
                    .last_heartbeat_time
                    .write()
                    .await
                    .insert(event.self_id, event.time);
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
            if let Some(enable) = MULTI_IMPL_SIDE.convert_self.get()
                && *enable
                && event.post_type == "message_sent"
            {
                event.post_type = "message".into();
            }

            SdkSide::broadcast_message(event).await?;
        }
        if let Ok(resposne) = serde_json::from_str::<ApiResponse>(msg) {
            // TODO 判断是否由内部发出Api的响应消息
            // if let Some(echo) = resposne.echo.clone() {
            //     if let Some(tx) = SINGLE_IMPL_SIDE.echo_tx_map.read().await.get(&echo) {
            //         info!("receive internal api response");
            //         tx.send(resposne).await?;
            //     } else {
            //         SdkSide::response_message(resposne).await?;
            //     }
            // }
        }
        Ok(())
    }

    /// 检测心跳
    async fn heartbeat_active(user_id: i64) {
        info!("start heartbeat active task");
        let heartbeat_time = *MULTI_IMPL_SIDE.heartbeat_interval.get().unwrap();
        loop {
            if let Some(last_heartbeat) = MULTI_IMPL_SIDE.last_heartbeat_time.write().await.get(&user_id) {
                let last_heartbeat = UNIX_EPOCH + Duration::from_secs(*last_heartbeat as u64);
                match SystemTime::now().duration_since(last_heartbeat) {
                    Ok(duration) if duration > Duration::from_secs(heartbeat_time as u64) => {
                        // 触发重连逻辑
                        // res 为 true 证明心跳超时，需要重连
                        info!(
                            "last_heartbeat: {:?}, heartbeat_time: {}, duration: {:?}, try to reconnect",
                            last_heartbeat, heartbeat_time, duration
                        );
                        Self::send_active_check_message(user_id).await;
                        info!("{} heartbeat timeout, try to reconnect", user_id);
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
        }
        info!("heartbeat active task exit")
    }

    /// 检查登录号状态是否存活
    async fn active_check(user_id: i64) {
        info!("start active check task");
        let mut interval = tokio::time::interval(time::Duration::from_secs(3600)); // 每小时触发一次

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    info!("check account login state");
                    Self::send_active_check_message(user_id).await;
                }
                _ = ctrl_c_signal!() => {
                    info!("receive interupt signal, exit active_check");
                    break;
                }
                _ = async { while *MULTI_IMPL_SIDE.disconnect.read().await.get(&user_id).unwrap_or(&false) { tokio::time::sleep(time::Duration::from_millis(100)).await; } } => {
                    info!("active flag set to false, exit active_check");
                    break;
                }
            }
        }
        info!("active check task exit");
    }

    /// 发送存活请求到协议端
    async fn send_active_check_message(user_id: i64) {
        let echo = generate_random_string(4);
        let api = Api {
            action: "get_status".into(),
            params: HashMap::new(),
            echo: Some(echo.clone()),
        };
        // let (tx, mut rx) = mpsc::channel::<ApiResponse>(1);
        // let mut echo_tx_map_writer = SINGLE_IMPL_SIDE.echo_tx_map.write().await;
        // echo_tx_map_writer.insert(echo.clone(), tx);
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
                    // active.store(false, Ordering::SeqCst);
                } else {
                    // 如果断开连接状态位为 false 则将其设置为 true，以便在连接中断时发送邮件提醒
                    // if !SINGLE_IMPL_SIDE.disconnect.load(Ordering::SeqCst) {
                    //     SINGLE_IMPL_SIDE.disconnect.store(true, Ordering::SeqCst);
                    // }
                }
            }
        });
        // 如果请求发送失败则移除该通道
        // if Self::send(api).await.is_err() {
        //     error!("send get_status api failed");
        //     echo_tx_map_writer.remove(&echo);
        // } else {
        //     info!("send get_status api success");
        // }
    }
}
