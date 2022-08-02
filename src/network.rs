use log::{debug, info};
use tokio_socks::tcp::Socks5Stream;
use futures_util::StreamExt;

use tokio::net::TcpStream;
type WebSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WebSocketTor = tokio_tungstenite::WebSocketStream<Socks5Stream<tokio::net::TcpStream>>;

pub enum Network {
    Clearnet,
    Tor,
}

pub async fn get_connection(
    config: &tostr::utils::Config,
    network: &Network,
) -> (tostr::Sink, tostr::StreamType) {
    match network {
        Network::Tor => {
            let ws_stream = connect_proxy(&config.relay).await;
            let (sink, stream) = ws_stream.split();
            let sink = tostr::Sink {
                sink: tostr::SinkType::Tor(std::sync::Arc::new(tokio::sync::Mutex::new(sink))),
                peer_addr: config.relay.clone(),
            };
            (sink, tostr::StreamType::Tor(stream))
        }
        Network::Clearnet => {
            let ws_stream = connect(&config.relay).await;
            let (sink, stream) = ws_stream.split();
            let sink = tostr::Sink {
                sink: tostr::SinkType::Clearnet(std::sync::Arc::new(tokio::sync::Mutex::new(sink))),
                peer_addr: config.relay.clone(),
            };
            (sink, tostr::StreamType::Clearnet(stream))
        }
    }
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
    let onion_addr = onion_addr.split("/").collect::<Vec<_>>()[2];
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
