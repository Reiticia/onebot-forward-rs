mod multi;
mod single;

use std::sync::OnceLock;

use futures_util::stream::{SplitSink, SplitStream};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::{
    config::WebSocketConfig,
    model::onebot::Api,
    wss::r#impl::{multi::MultiImplSide, single::SingleImplSide},
};

pub type Writer = SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>;
pub type Reader = SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>;

#[derive(Debug, Default)]
pub struct ImplSide;

static CONNECT_MODEL: OnceLock<bool> = OnceLock::new();

pub trait ImplSideTrait {
    /// 建立连接
    async fn connect(websocket: &WebSocketConfig, convert_self: Option<bool>) -> anyhow::Result<()>;
    /// 判断协议端是否存活
    async fn alive() -> Vec<Option<i64>>;
    /// 发送消息
    async fn send(data: Api) -> anyhow::Result<()>;
}

impl ImplSideTrait for ImplSide {
    async fn connect(websocket: &WebSocketConfig, convert_self: Option<bool>) -> anyhow::Result<()> {
        if websocket.is_multi_connect_model() {
            CONNECT_MODEL.set(true).ok();
            MultiImplSide::connect(websocket, convert_self).await
        } else {
            CONNECT_MODEL.set(false).ok();
            SingleImplSide::connect(websocket, convert_self).await
        }
    }

    /// 判断协议端是否存活
    async fn alive() -> Vec<Option<i64>> {
        let connect_model = CONNECT_MODEL.get().unwrap();
        if *connect_model {
            MultiImplSide::alive().await
        } else {
            SingleImplSide::alive().await
        }
    }

    /// 发送消息
    async fn send(data: Api) -> anyhow::Result<()> {
        let connect_model = CONNECT_MODEL.get().unwrap();
        if *connect_model {
            MultiImplSide::send(data).await
        } else {
            SingleImplSide::send(data).await
        }
    }
}
