use log::{debug, info};
use nostr_bot::FunctorType;

mod bot;
mod simpledb;
mod twitter;
mod utils;

use bot::MyState;

#[tokio::main]
async fn main() {
    nostr_bot::init_logger();

    let args = std::env::args().collect::<Vec<String>>();
    if args.len() != 2 {
        println!("Usage: {} --clearnet|--tor", args[0]);
        std::process::exit(1);
    }
    let network = match args[1].as_str() {
        "--clearnet" => nostr_bot::Network::Clearnet,
        "--tor" => nostr_bot::Network::Tor,
        _ => panic!("Incorrect network settings"),
    };

    let config_path = std::path::PathBuf::from("config");
    let config = utils::parse_config(&config_path);
    debug!("{:?}", config);

    info!("Starting bot");

    let secp = secp256k1::Secp256k1::new();
    let keypair = secp256k1::KeyPair::from_seckey_str(&secp, &config.secret).unwrap();

    let sender = nostr_bot::new_sender();

    let (tx, rx) = tokio::sync::mpsc::channel::<bot::ConnectionMessage>(64);
    type State = nostr_bot::State<MyState>;
    let state = nostr_bot::wrap_state(MyState {
        config: config.clone(),
        sender: sender.clone(),
        db: std::sync::Arc::new(std::sync::Mutex::new(simpledb::SimpleDatabase::from_file(
            "data/users".to_string(),
        ))),
        error_sender: tx.clone(),
    });

    let start_existing = {
        let state = state.clone();

        async move {
            let state = state.lock().await;
            bot::start_existing(
                state.db.clone(),
                &state.config,
                state.sender.clone(),
                state.error_sender.clone(),
            )
            .await;
        }
    };

    let error_listener = {
        let state = state.clone();
        let sender = state.lock().await.sender.clone();

        async move {
            bot::error_listener(rx, sender, keypair).await;
        }
    };

    let mut bot = nostr_bot::Bot::<State>::new(keypair, config.relays, network)
        .name(&config.name)
        .about(&config.about)
        .picture(&config.picture_url)
        .intro_message(&config.hello_message)
        .command(
            nostr_bot::Command::new("!add", nostr_bot::wrap!(bot::handle_add))
                .desc("Add new account to be followed by the bot."),
        )
        .command(
            nostr_bot::Command::new("!random", nostr_bot::wrap!(bot::handle_random))
                .desc("Returns random account the bot is following."),
        )
        .command(
            nostr_bot::Command::new("!list", nostr_bot::wrap!(bot::handle_list))
                .desc("Returns list of all accounts that the bot follows."),
        )
        .command(
            nostr_bot::Command::new("!relays", nostr_bot::wrap_extra!(bot::handle_relays))
                .desc("Show connected relay."),
        )
        .help()
        .sender(sender)
        .spawn(Box::pin(start_existing))
        .spawn(Box::pin(error_listener));

    bot.run(state).await;
}
