use log::{debug, info};

mod bot;
mod network;
mod nostr;
mod simpledb;
mod twitter;
mod utils;

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
        "--clearnet" => network::Network::Clearnet,
        "--tor" => network::Network::Tor,
        _ => panic!("Incorrect network settings"),
    };

    let config_path = std::path::PathBuf::from("config");
    let config = utils::parse_config(&config_path);
    debug!("{:?}", config);

    info!("Starting bot");
    // TODO: Use tokio Mutex?
    let db = simpledb::SimpleDatabase::from_file("data/users".to_string());
    let db = std::sync::Arc::new(std::sync::Mutex::new(db));

    let mut first_connection = true;

    // TODO: Don't send Hi message in a loop
    // Also set profiles only once when new users are created
    loop {
        // TODO: Start tor service, add iptables settings to the Dockerfile
        let (sink, stream) = network::get_connection(&config, &network).await;

        let secp = secp256k1::Secp256k1::new();
        let keypair = secp256k1::KeyPair::from_seckey_str(&secp, &config.secret).unwrap();

        if first_connection {
            first_connection = false;
            bot::introduction(&config, &keypair, sink.clone()).await;
        }

        bot::run(keypair, sink, stream, db.clone(), config.clone()).await;

        let wait_secs = 30;
        info!(
            "Connection lost. Will try to reconnect in {} seconds",
            wait_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
    }
}
