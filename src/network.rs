use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use log::{debug, info};
use tokio_socks::tcp::Socks5Stream;

use futures_util::stream::{SplitSink, SplitStream};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;

use crate::utils;

type WebSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WebSocketTor = tokio_tungstenite::WebSocketStream<Socks5Stream<tokio::net::TcpStream>>;

type SplitSinkClearnet = futures_util::stream::SplitSink<
    WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tungstenite::Message,
>;
type Stream = futures_util::stream::SplitStream<
    WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

type SplitSinkTor = SplitSink<
    WebSocketStream<Socks5Stream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;
type StreamTor = SplitStream<WebSocketStream<Socks5Stream<tokio::net::TcpStream>>>;

#[derive(Clone, Debug)]
pub enum SinkType {
    Clearnet(std::sync::Arc<tokio::sync::Mutex<SplitSinkClearnet>>),
    Tor(std::sync::Arc<tokio::sync::Mutex<SplitSinkTor>>),
}

#[derive(Debug)]
pub enum StreamType {
    Clearnet(Stream),
    Tor(StreamTor),
}

#[derive(Clone)]
pub struct Sink {
    pub sink: SinkType,
    pub peer_addr: String,
}

pub enum Network {
    Clearnet,
    Tor,
}

pub async fn send(msg: String, sink_wrap: Sink) {
    match sink_wrap.sink {
        SinkType::Clearnet(sink) => {
            debug!("Sending >{}< to {} over clearnet", msg, sink_wrap.peer_addr);
            sink.lock()
                .await
                .send(tungstenite::Message::Text(msg))
                .await
                .unwrap()
        }
        SinkType::Tor(sink) => {
            debug!("Sending >{}< to {} over tor", msg, sink_wrap.peer_addr);
            sink.lock()
                .await
                .send(tungstenite::Message::Text(msg))
                .await
                .unwrap()
        }
    }
}

pub async fn get_connection(relay: &String, network: &Network) -> (Sink, StreamType) {
    match network {
        Network::Tor => {
            let ws_stream = connect_proxy(relay).await;
            let (sink, stream) = ws_stream.split();
            let sink = Sink {
                sink: SinkType::Tor(std::sync::Arc::new(tokio::sync::Mutex::new(sink))),
                peer_addr: relay.clone(),
            };
            (sink, StreamType::Tor(stream))
        }
        Network::Clearnet => {
            let ws_stream = connect(relay).await;
            let (sink, stream) = ws_stream.split();
            let sink = Sink {
                sink: SinkType::Clearnet(std::sync::Arc::new(tokio::sync::Mutex::new(sink))),
                peer_addr: relay.clone(),
            };
            (sink, StreamType::Clearnet(stream))
        }
    }
}

pub async fn get_connections(
    config: &utils::Config,
    network: &Network,
) -> (Vec<Sink>, Vec<StreamType>) {
    let mut sinks = vec![];
    let mut streams = vec![];

    for relay in &config.relays {
        let (sink, stream) = get_connection(&relay, network).await;
        sinks.push(sink);
        streams.push(stream);
    }

    (sinks, streams)
}

async fn connect(relay: &String) -> WebSocket {
    info!("Connecting to {} using clearnet", relay);
    let (ws_stream, _response) = tokio_tungstenite::connect_async(url::Url::parse(relay).unwrap())
        .await
        .expect("Can't connect");
    ws_stream
}

const TCP_PROXY_ADDR: &str = "127.0.0.1:9050";

async fn connect_proxy(relay: &String) -> WebSocketTor {
    info!("Connecting to {} using tor", relay);
    let ws_onion_addr = relay;
    let onion_addr = ws_onion_addr.clone();
    let onion_addr = onion_addr.split('/').collect::<Vec<_>>()[2];
    debug!("onion_addr >{}<", onion_addr);
    let socket = TcpStream::connect(TCP_PROXY_ADDR).await.unwrap();
    socket.set_nodelay(true).unwrap();
    let conn = Socks5Stream::connect_with_socket(socket, onion_addr)
        .await
        .unwrap();

    let (ws_stream, _response) = tokio_tungstenite::client_async(ws_onion_addr, conn)
        .await
        .expect("tungsten failed");
    ws_stream
}
