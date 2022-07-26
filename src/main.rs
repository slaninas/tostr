use futures_util::StreamExt;
use log::{debug, info};
use tokio::{
    net::TcpStream,
};
use tokio_socks::tcp::Socks5Stream;

type WebSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WebSocketTor = tokio_tungstenite::WebSocketStream<Socks5Stream<tokio::net::TcpStream>>;

enum Network {
    Clearnet,
    Tor,
}

#[tokio::main]
async fn main() {
    let _start = std::time::Instant::now();
    env_logger::Builder::from_default_env()
        // .format(move |buf, rec| {
        // let t = start.elapsed().as_secs_f32();
        // writeln!(buf, "{:.03} [{}] - {}", t, rec.level(), rec.args())
        // })
        .init();

    let args = std::env::args().collect::<Vec<String>>();
    if args.len() != 2 {
        println!("Usage: {} --clearnet|--tor", args[0]);
        std::process::exit(1);
    }
    let network = match args[1].as_str() {
        "--clearnet" => Network::Clearnet,
        "--tor" => Network::Tor,
        _ => panic!("Incorrect network settings"),
    };


    let config_path = std::path::PathBuf::from("config");
    let config = tostr::utils::parse_config(&config_path);
    debug!("{:?}", config);

    info!("Starting bot");
    // TODO: Use tokio Mutex?
    let db = tostr::simpledb::SimpleDatabase::from_file("data/users".to_string());
    let db = std::sync::Arc::new(std::sync::Mutex::new(db));

    let mut first_connection = true;

    // TODO: Don't send Hi message in a loop
    // Also set profiles only once when new users are created
    loop {

        // TODO: Start tor service, add iptables settings to the Dockerfile
        let (sink, stream) = get_connection(&config, &network).await;

        let secp = secp256k1::Secp256k1::new();
        let keypair = secp256k1::KeyPair::from_seckey_str(&secp, &config.secret).unwrap();


        if first_connection {
            first_connection = false;
            tostr::introduction(&config, &keypair, sink.clone()).await;
        }

        tostr::run(keypair, sink, stream, db.clone(), config.clone()).await;

        let wait_secs = 30;
        info!(
            "Connection lost. Will try to reconnect in {} seconds",
            wait_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
    }
}

async fn get_connection(config: &tostr::utils::Config, network: &Network) -> (tostr::Sink, tostr::StreamType) {
        match network {
            Network::Tor => {
            let ws_stream = connect_proxy(&config.relay).await;
            let (sink, stream) = ws_stream.split();
            let sink = tostr::Sink {
                sink: tostr::SinkType::Tor(std::sync::Arc::new(tokio::sync::Mutex::new(
                    sink,
                ))),
                peer_addr: config.relay.clone(),
            };
            (sink, tostr::StreamType::Tor(stream))

        },
            Network::Clearnet => {
            let ws_stream = connect(&config.relay).await;
            let (sink, stream) = ws_stream.split();
            let sink = tostr::Sink {
                sink: tostr::SinkType::Clearnet(std::sync::Arc::new(tokio::sync::Mutex::new(
                    sink,
                ))),
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
