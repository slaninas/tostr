use futures_util::StreamExt;
use log::{debug, info, warn};
use std::fmt::Write;

use rand::Rng;

use crate::network;
use crate::nostr;
use crate::simpledb;
use crate::twitter;
use crate::utils;

type Receiver = tokio::sync::mpsc::Receiver<ConnectionMessage>;
type Sender = tokio::sync::mpsc::Sender<ConnectionMessage>;

type NostrMessageReceiver = tokio::sync::mpsc::Receiver<nostr::Message>;
type NostrMessageSender = tokio::sync::mpsc::Sender<nostr::Message>;

#[derive(PartialEq, Debug)]
enum ConnectionStatus {
    Success,
    Failed,
}

#[derive(Debug)]
pub struct ConnectionMessage {
    status: ConnectionStatus,
    timestamp: std::time::SystemTime,
}

pub async fn run(
    keypair: secp256k1::KeyPair,
    sinks: Vec<network::Sink>,
    streams: Vec<network::Stream>,
    db: simpledb::Database,
    config: utils::Config,
) -> tokio::task::JoinHandle<()> {
    let (tx, rx) = tokio::sync::mpsc::channel::<ConnectionMessage>(64);
    start_existing(db.clone(), &config, sinks.clone(), tx.clone());

    let s = sinks.clone();
    tokio::spawn(async move {
        error_listener(rx, s.clone(), keypair).await;
    });

    let (main_bot_tx, main_bot_rx) = tokio::sync::mpsc::channel::<nostr::Message>(64);

    for (id, stream) in streams.into_iter().enumerate() {
        let sink = sinks[id].clone();
        let main_bot_tx = main_bot_tx.clone();
        tokio::spawn(async move {
            listen_relay(stream, sink, main_bot_tx, &keypair).await;
        });
    }

    tokio::spawn(async move {
        main_bot_listener(
            db.clone(),
            sinks,
            tx.clone(),
            main_bot_rx,
            &keypair,
            &config,
        )
        .await;
    })
}

async fn main_bot_listener(
    db: simpledb::Database,
    sinks: Vec<network::Sink>,
    error_sender: Sender,
    mut rx: NostrMessageReceiver,
    keypair: &secp256k1::KeyPair,
    config: &utils::Config,
) {
    let mut handled_events = std::collections::HashSet::new();

    info!("Main bot listener started.");
    while let Some(message) = rx.recv().await {
        let event_id = message.content.id.clone();
        if handled_events.contains(&event_id) {
            debug!("Event with id={} already handled, ignoring.", event_id);
            continue;
        }

        handled_events.insert(event_id);

        let error_sender = error_sender.clone();

        match handle_command(
            message.content,
            db.clone(),
            sinks.clone(),
            error_sender.clone(),
            config,
        )
        .await
        {
            Ok(response) => {
                network::send_to_all(response.sign(keypair).format(), sinks.clone()).await
            }
            Err(e) => debug!("{}", e),
        }
    }
}

async fn listen_relay(
    stream: network::Stream,
    sink: network::Sink,
    main_bot_tx: NostrMessageSender,
    main_bot_keypair: &secp256k1::KeyPair,
) {
    info!("Relay listener for {} started.", sink.peer_addr);
    let peer_addr = sink.peer_addr.clone();

    let network_type = match sink.clone().sink {
        network::SinkType::Clearnet(_) => network::Network::Clearnet,
        network::SinkType::Tor(_) => network::Network::Tor,
    };

    let mut stream = stream;
    let mut sink = sink;

    loop {
        relay_listener(stream, sink.clone(), main_bot_tx.clone(), main_bot_keypair).await;
        let wait = std::time::Duration::from_secs(30);
        warn!(
            "Connection with {} lost, I will try to reconnect in {:?}",
            peer_addr, wait
        );

        // Reconnect
        loop {
            tokio::time::sleep(wait).await;
            let connection = network::get_connection(&peer_addr, &network_type).await;
            match connection {
                Ok((new_sink, new_stream)) => {
                    sink.update(new_sink.sink).await;
                    stream = new_stream;
                    break;
                }
                Err(_) => warn!(
                    "Relay listener is unable to reconnect to {}. Will try again in {:?}",
                    peer_addr, wait
                ),
            }
        }
    }
}

async fn relay_listener(
    stream: network::Stream,
    sink: network::Sink,
    main_bot_tx: NostrMessageSender,
    main_bot_keypair: &secp256k1::KeyPair,
) {
    request_subscription(main_bot_keypair, sink.clone()).await;

    let listen = |message: Result<tungstenite::Message, tungstenite::Error>| async {
        let data = match message {
            Ok(data) => data,
            Err(error) => {
                info!("Stream read failed: {}", error);
                return;
            }
        };

        let data_str = data.to_string();
        debug!("Got message >{}< from {}.", data_str, stream.peer_addr);

        match serde_json::from_str::<nostr::Message>(&data.to_string()) {
            Ok(message) => {
                debug!(
                    "Sending message with event id={} to master bot",
                    message.content.id
                );
                match main_bot_tx.send(message).await {
                    Ok(_) => {}
                    Err(e) => panic!("Error sending message to main bot: {}", e),
                }
            }
            Err(e) => {
                debug!("Unable to parse message: {}", e);
            }
        }
    };

    match stream.stream {
        network::StreamType::Clearnet(stream) => {
            let f = stream.for_each(listen);
            f.await;
        }
        network::StreamType::Tor(stream) => {
            let f = stream.for_each(listen);
            f.await;
        }
    }
}

async fn error_listener(mut rx: Receiver, sinks: Vec<network::Sink>, keypair: secp256k1::KeyPair) {
    // If the message of the same kind as last one was received in less than this, discard it to
    // prevent spamming
    let discard_period = std::time::Duration::from_secs(3600);

    let mut last_accepted_message = ConnectionMessage {
        status: ConnectionStatus::Success,
        timestamp: std::time::SystemTime::now() - discard_period,
    };

    while let Some(message) = rx.recv().await {
        let mut message_to_send = std::option::Option::<String>::None;

        if message.status != last_accepted_message.status {
            match message.status {
                ConnectionStatus::Success => {
                    message_to_send = Some("Connection to Twitter reestablished! :)".to_string());
                }
                ConnectionStatus::Failed => {
                    message_to_send = Some("I can't connect to Twitter right now :(.".to_string());
                }
            }

            last_accepted_message = message;
        } else {
            let duration_since_last_accepted = message
                .timestamp
                .duration_since(last_accepted_message.timestamp)
                .unwrap();

            debug!(
                "Since last accepted message: {:?}, discard period: {:?}",
                duration_since_last_accepted, discard_period
            );

            if duration_since_last_accepted >= discard_period {
                match message.status {
                    ConnectionStatus::Success => {}
                    ConnectionStatus::Failed => {
                        message_to_send =
                            Some("I'm still unable to connect to Twitter :(".to_string());
                    }
                }
                last_accepted_message = message;
            }
        }

        if let Some(message_to_send) = message_to_send {
            let event = nostr::EventNonSigned {
                created_at: utils::unix_timestamp(),
                kind: 1,
                tags: vec![],
                content: message_to_send,
            }
            .sign(&keypair);

            network::send_to_all(event.format(), sinks.clone()).await;
        }
    }
}

async fn handle_command(
    event: nostr::Event,
    db: simpledb::Database,
    sinks: Vec<network::Sink>,
    tx: Sender,
    config: &utils::Config,
) -> Result<nostr::EventNonSigned, String> {
    let command = &event.content;

    let response = if command.starts_with("add ") {
        Ok(handle_add(db, event, sinks, tx, config).await)
    } else if command.starts_with("random") {
        Ok(handle_random(db, event).await)
    } else if command.starts_with("list") {
        Ok(handle_list(db, event).await)
    } else if command.starts_with("relays") {
        Ok(handle_relays(sinks.clone(), event).await)
    } else {
        Err(format!("Unknown command >{}<", command))
    };
    response
}

async fn handle_relays(sinks: Vec<network::Sink>, event: nostr::Event) -> nostr::EventNonSigned {
    let mut text = "Right now I'm connected to these relays:\\n".to_string();

    for sink in sinks {
        let peer_addr = sink.peer_addr.clone();
        if network::ping(sink).await {
            write!(text, "{}\\n", peer_addr).unwrap();
        }
    }

    let tags = nostr::get_tags_for_reply(event);
    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: text,
    }
}

async fn handle_list(db: simpledb::Database, event: nostr::Event) -> nostr::EventNonSigned {
    let follows = db.lock().unwrap().get_follows();
    let mut usernames = follows.keys().collect::<Vec<_>>();
    usernames.sort();

    let mut tags = nostr::get_tags_for_reply(event);
    let orig_tags_count = tags.len();

    let mut text = format!("Hi, I'm following {} accounts:\\n", usernames.len());
    for (index, &username) in usernames.iter().enumerate() {
        let secret = follows.get(username).unwrap();
        tags.push(vec![
            "p".to_string(),
            secret.x_only_public_key().0.to_string(),
        ]);
        write!(text, "#[{}]\\n", index + orig_tags_count).unwrap();
    }

    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: text,
    }
}

async fn handle_random(db: simpledb::Database, event: nostr::Event) -> nostr::EventNonSigned {
    let follows = db.lock().unwrap().get_follows();

    if follows.is_empty() {
        return nostr::EventNonSigned {
            created_at: utils::unix_timestamp(),
            kind: 1,
            tags: nostr::get_tags_for_reply(event),
            content: format!(
                "Hi, there are no accounts. Try to add some using 'add twitter_username' command."
            ),
        };
    }

    let index = rand::thread_rng().gen_range(0..follows.len());

    let random_username = follows.keys().collect::<Vec<_>>()[index];

    let secret = follows.get(random_username).unwrap();

    let mut tags = nostr::get_tags_for_reply(event);
    tags.push(vec![
        "p".to_string(),
        secret.x_only_public_key().0.to_string(),
    ]);
    let mention_index = tags.len() - 1;

    debug!("Command random: returning {}", random_username);
    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: format!("Hi, random account to follow: #[{}]", mention_index),
    }
}

async fn handle_add(
    db: simpledb::Database,
    event: nostr::Event,
    sinks: Vec<network::Sink>,
    tx: Sender,
    config: &utils::Config,
) -> nostr::EventNonSigned {
    let username = event.content[4..event.content.len()]
        .to_ascii_lowercase()
        .replace('@', "");

    if db.clone().lock().unwrap().contains_key(&username) {
        let keypair = simpledb::get_user_keypair(&username, db);
        let (pubkey, _parity) = keypair.x_only_public_key();
        debug!(
            "User {} already added before. Sending existing pubkey {}",
            username, pubkey
        );
        return get_handle_response(event, &pubkey.to_string());
    }

    if db.lock().unwrap().follows_count() + 1 > config.max_follows {
        return nostr::EventNonSigned {
            created_at: utils::unix_timestamp(),
            kind: 1,
            tags: nostr::get_tags_for_reply(event),
            content: format!("Hi, sorry, couldn't add new account. I'm already running at my max capacity ({} users).", config.max_follows),
        };
    }

    if !twitter::user_exists(&username).await {
        return nostr::EventNonSigned {
            created_at: utils::unix_timestamp(),
            kind: 1,
            tags: nostr::get_tags_for_reply(event),
            content: format!("Hi, I wasn't able to find {} on Twitter :(.", username),
        };
    }

    let keypair = utils::get_random_keypair();

    db.lock()
        .unwrap()
        .insert(username.clone(), keypair.display_secret().to_string())
        .unwrap();
    let (xonly_pubkey, _) = keypair.x_only_public_key();
    let username = username.to_string();
    info!(
        "Starting worker for username {}, pubkey {}",
        username, xonly_pubkey
    );

    {
        let sinks = sinks.clone();
        let refresh_interval_secs = config.refresh_interval_secs;
        tokio::spawn(async move {
            update_user(username, &keypair, sinks, tx, refresh_interval_secs).await;
        });
    }

    get_handle_response(event, &xonly_pubkey.to_string())
}

fn get_handle_response(event: nostr::Event, new_bot_pubkey: &str) -> nostr::EventNonSigned {
    let mut all_tags = nostr::get_tags_for_reply(event);
    all_tags.push(vec!["p".to_string(), new_bot_pubkey.to_string()]);
    let last_tag_position = all_tags.len() - 1;

    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags: all_tags,
        content: format!(
            "Hi, tweets will be forwarded to nostr by #[{}].",
            last_tag_position
        ),
    }
}

pub async fn introduction(
    config: &utils::Config,
    keypair: &secp256k1::KeyPair,
    sink: network::Sink,
) {
    // info!("Main bot is sending set_metadata >{}<
    // Set profile
    info!(
        "main bot is settings name: \"{}\", about: \"{}\", picture_url: \"{}\"",
        config.name, config.about, config.picture_url
    );
    let event = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{\"name\":\"{}\",\"about\":\"{}\",\"picture\":\"{}\"}}"#,
            config.name, config.about, config.picture_url
        ),
    );

    network::send(event.format(), sink.clone()).await;

    // Say hi
    let welcome = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        1,
        vec![],
        config.hello_message.clone(),
    );

    info!("main bot is sending message \"{}\"", config.hello_message);
    network::send(welcome.format(), sink.clone()).await;
}

async fn request_subscription(keypair: &secp256k1::KeyPair, sink: network::Sink) {
    let random_string = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(64)
        .collect::<Vec<_>>();
    let random_string = String::from_utf8(random_string).unwrap();
    // Listen for my pubkey mentions
    network::send(
        format!(
            r##"["REQ", "{}", {{"#p": ["{}"], "since": {}}} ]"##,
            random_string,
            keypair.x_only_public_key().0,
            utils::unix_timestamp(),
        ),
        sink,
    )
    .await;
}

pub fn start_existing(
    db: simpledb::Database,
    config: &utils::Config,
    sinks: Vec<network::Sink>,
    tx: Sender,
) {
    for (username, keypair) in db.lock().unwrap().get_follows() {
        let tx = tx.clone();
        info!("Starting worker for username {}", username);

        {
            let refresh = config.refresh_interval_secs;
            let sinks = sinks.clone();
            tokio::spawn(async move {
                update_user(username, &keypair, sinks, tx, refresh).await;
            });
        }
    }
}

#[allow(dead_code)]
async fn fake_worker(username: String, refresh_interval_secs: u64) {
    loop {
        debug!(
            "Fake worker for user {}  is going to sleep for {} s",
            username, refresh_interval_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;
        debug!("Faking the work for user {}", username);
    }
}

pub async fn update_user(
    username: String,
    keypair: &secp256k1::KeyPair,
    sinks: Vec<network::Sink>,
    tx: Sender,
    refresh_interval_secs: u64,
) {
    // fake_worker(username, refresh_interval_secs).await;
    // return;

    let pic_url = twitter::get_pic_url(&username).await;
    let event = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{\"name\":\"tostr_{}\",\"about\":\"Tweets forwarded from https://twitter.com/{} by [tostr](https://github.com/slaninas/tostr) bot.\",\"picture\":\"{}\"}}"#,
            username, username, pic_url
        ),
    );

    network::send_to_all(event.format(), sinks.clone()).await;

    let mut since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();

    loop {
        debug!(
            "Worker for @{} is going to sleep for {} s",
            username, refresh_interval_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;

        let until = std::time::SystemTime::now().into();
        let new_tweets = twitter::get_new_tweets(&username, since, until).await;

        match new_tweets {
            Ok(new_tweets) => {
                // --since seems to be inclusive and --until exclusive so this should be fine
                since = until;

                // twint returns newest tweets first, reverse the Vec here so that tweets are send to relays
                // in order they were published. Still the created_at field can easily be the same so in the
                // end it depends on how the relays handle it
                for tweet in new_tweets.iter().rev() {
                    network::send_to_all(
                        twitter::get_tweet_event(tweet).sign(keypair).format(),
                        sinks.clone(),
                    )
                    .await;
                }

                tx.send(ConnectionMessage {
                    status: ConnectionStatus::Success,
                    timestamp: std::time::SystemTime::now(),
                })
                .await
                .unwrap();
            }
            Err(e) => {
                tx.send(ConnectionMessage {
                    status: ConnectionStatus::Failed,
                    timestamp: std::time::SystemTime::now(),
                })
                .await
                .unwrap();
                warn!("{}", e);
            }
        }
        // break;
    }
}
