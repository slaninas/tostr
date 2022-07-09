use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use log::{debug, info};

use rand::Rng;

use tokio_tungstenite::WebSocketStream;

pub mod nostr;
pub mod simpledb;
pub mod utils;

type SplitSink = futures_util::stream::SplitSink<
    WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tungstenite::Message,
>;
type Stream = futures_util::stream::SplitStream<
    WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

type WrappedSink = std::sync::Arc<tokio::sync::Mutex<SplitSink>>;

#[derive(Clone)]
pub struct Sink {
    pub sink: WrappedSink,
    pub peer_addr: String,
}

pub async fn run(
    keypair: secp256k1::KeyPair,
    sink: Sink,
    stream: Stream,
    db: simpledb::Database,
    config: utils::Config,
) {
    // Set profile
    let event = nostr::Event::new(
        &keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{\"name\":\"tostr_bot\",\"about\":\"Hi, I'm [tostr](https://github.com/slaninas/tostr) bot. Reply to me with 'add @twitter_account' or 'random'.\",\"picture\":\"https://st2.depositphotos.com/1187563/7129/i/450/depositphotos_71295829-stock-photo-old-style-photo-toast-popping.jpg\"}}"#,
        ),
    );

    send(event.format(), sink.clone()).await;
    let welcome = nostr::Event::new(
        &keypair,
        utils::unix_timestamp(),
        1,
        vec![],
        "Hi, I'm tostr, reply with command 'add twitter_account'".to_string(),
    );

    send(welcome.format(), sink.clone()).await;

    // Listen for my pubkey mentions
    send(
        format!(
            r##"["REQ", "{}", {{"#p": ["{}"], "since": {}}} ]"##,
            "dsfasdfdafadf",
            keypair.x_only_public_key().0,
            utils::unix_timestamp(),
        ),
        sink.clone(),
    )
    .await;

    start_existing(db.clone(), &config, sink.clone());

    let f = stream.for_each(|message| async {
        let data = message.unwrap();

        let data_str = data.to_string();
        debug!("Got message >{}<", data_str);

        match serde_json::from_str::<nostr::Message>(&data.to_string()) {
            Ok(message) => {
                match handle_command(
                    message.content,
                    db.clone(),
                    sink.clone(),
                    config.refresh_interval_secs,
                )
                .await
                {
                    Ok(response) => send(response.sign(&keypair).format(), sink.clone()).await,
                    Err(e) => debug!("{}", e),
                }
            }
            Err(e) => {
                debug!("Unable to parse message: {}", e);
            }
        }
    });

    f.await;
}

async fn handle_command(
    event: nostr::Event,
    db: simpledb::Database,
    sink: Sink,
    refresh_interval_secs: u64,
) -> Result<nostr::EventNonSigned, String> {
    let command = &event.content;

    let response = if command.starts_with("add ") {
        Ok(handle_add(db, event, sink, refresh_interval_secs).await)
    } else if command.starts_with("random") {
        Ok(handle_random(db, event).await)
    } else {
        Err(format!("Unknown command >{}<", command))
    };
    response
}

async fn handle_random(db: simpledb::Database, event: nostr::Event) -> nostr::EventNonSigned {
    let follows = db.lock().unwrap().get_follows();
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
        tags: tags,
        content: format!(
            "Hi, random account to follow: @{} with pubkey #[{}]",
            random_username, mention_index
        ),
    }
}

async fn handle_add(
    db: simpledb::Database,
    event: nostr::Event,
    sink: Sink,
    refresh_interval_secs: u64,
) -> nostr::EventNonSigned {
    let username = event.content[5..event.content.len()].to_string();

    if db.clone().lock().unwrap().contains_key(&username) {
        let keypair = simpledb::get_user_keypair(&username, db);
        let (pubkey, _parity) = keypair.x_only_public_key();
        debug!(
            "User @{} already added before. Sending existing pubkey {}",
            username, pubkey
        );
        return get_handle_response(event, &pubkey.to_string());
    }
    let keypair = utils::get_random_keypair();

    db.clone()
        .lock()
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
        let sink = sink.clone();
        tokio::spawn(async move {
            update_user(username, &keypair, sink, refresh_interval_secs).await;
        });
    }

    get_handle_response(event, &xonly_pubkey.to_string())
}

fn get_handle_response(
    event: nostr::Event,
    new_bot_pubkey: &str,
) -> nostr::EventNonSigned {
    let mut all_tags = nostr::get_tags_for_reply(event);
    all_tags.push(vec!["p".to_string(), new_bot_pubkey.to_string()]);
    let last_tag_position = all_tags.len() - 1;

    nostr::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags: all_tags,
        content: format!("Hi, tweets will be forwarded to nostr by #[{}].", last_tag_position),
    }
}

async fn send(msg: String, sink: Sink) {
    debug!("Sending >{}< to {}", msg, sink.peer_addr);
    sink.sink
        .lock()
        .await
        .send(tungstenite::Message::Text(msg))
        .await
        .unwrap();
}

pub fn start_existing(db: simpledb::Database, config: &utils::Config, sink: Sink) {
    for (username, keypair) in db.lock().unwrap().get_follows() {
        info!("Starting worker for username {}", username);

        {
            let refresh = config.refresh_interval_secs.clone();
            let sink = sink.clone();
            tokio::spawn(async move {
                update_user(username, &keypair, sink, refresh).await;
            });
        }
    }
}

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
    sink: Sink,
    refresh_interval_secs: u64,
) {
    // fake_worker(username, refresh_interval_secs).await;
    // return;
    let event = nostr::Event::new(
        keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{\"name\":\"tostr_{}\",\"about\":\"Tweets forwarded from https://twitter.com/{} by [tostr](https://github.com/slaninas/tostr) bot.\",\"picture\":\"https://upload.wikimedia.org/wikipedia/commons/thumb/7/74/Twitter_Logo_Mini.svg/234px-Twitter_Logo_Mini.svg.png\"}}"#,
            username, username
        ),
    );

    send(event.format(), sink.clone()).await;
    let mut since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();
    loop {
        debug!(
            "Worker for @{} is going to sleep for {} s",
            username, refresh_interval_secs
        );
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;

        let new_tweets = utils::get_new_tweets(&username, since).await;
        since = std::time::SystemTime::now().into();

        // twint returns newest tweets first, reverse the Vec here so that tweets are send to relays
        // in order they were published. Still the created_at field can easily be the same so in the
        // end it depends on how the relays handle it
        for tweet in new_tweets.iter().rev() {
            sink.sink
                .clone()
                .lock()
                .await
                .send(tungstenite::Message::Text(
                    utils::get_tweet_event(tweet).sign(&keypair).format(),
                ))
                .await
                .unwrap();
        }
        // break;
    }
}
