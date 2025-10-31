use super::Writer;
use crate::wss::r#impl::ImplSideTrait;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, OnceLock, atomic::AtomicBool},
};

use log::{info, warn};
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

use crate::{
    config::{self, WebSocketConfig},
    model::onebot::Api,
};

#[derive(Debug, Default)]
pub struct MultiImplSide {
    writers: HashMap<i64, Writer>,
}

static MULTI_IMPL_SIDE: LazyLock<RwLock<MultiImplSide>> = LazyLock::new(|| RwLock::new(MultiImplSide::default()));
static LAST_HEARTBEAT_TIME: LazyLock<RwLock<HashMap<i64, i64>>> = LazyLock::new(|| RwLock::new(HashMap::new()));
static CONVERT_SELF: OnceLock<bool> = OnceLock::new();
static HEARTBEAT_INTERVAL: OnceLock<i64> = OnceLock::new();
/// 是否为连接断开，用于决定在连接断开后重连时是否发送邮件
static DISCONNECT: AtomicBool = AtomicBool::new(false);

impl ImplSideTrait for MultiImplSide {
    async fn connect(websocket: &WebSocketConfig, convert_self: Option<bool>) -> anyhow::Result<()> {
        info!("start websocket multi client mode");
        CONVERT_SELF.get_or_init(|| convert_self.unwrap_or_default());
        HEARTBEAT_INTERVAL.get_or_init(|| websocket.heartbeat);
        let is_notice = config::APP_CONFIG.get_notice();
        info!("try to connect to servers");
        let mut websockets = Vec::new();
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
            websockets.push(ws);
        }
        // 创建一个用于终止所有连接的异步通知任务
        // let signal_task = async move {};
        // loop {}

        // Ok(())
        todo!()
    }

    /// 判断协议端是否存活
    async fn alive() -> Option<i64> {
        unimplemented!()
    }

    /// 发送消息
    async fn send(data: Api) -> anyhow::Result<()> {
        unimplemented!()
    }
}

impl MultiImplSide {}
