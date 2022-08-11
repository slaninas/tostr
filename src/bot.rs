use log::{debug, info, warn};
use std::fmt::Write;

use rand::Rng;

use crate::simpledb;
use crate::twitter;
use crate::utils;

type Receiver = tokio::sync::mpsc::Receiver<ConnectionMessage>;
type ErrorSender = tokio::sync::mpsc::Sender<ConnectionMessage>;

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

pub struct MyState {
    pub config: utils::Config,
    pub db: simpledb::Database,
    pub sender: nostr_bot::Sender,

    // error_receiver: tokio::sync::mpsc::Receiver<bot::ConnectionMessage>,
    pub error_sender: tokio::sync::mpsc::Sender<ConnectionMessage>,
}

pub async fn error_listener(
    mut rx: Receiver,
    sender: nostr_bot::Sender,
    keypair: secp256k1::KeyPair,
) {
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
            let event = nostr_bot::EventNonSigned {
                created_at: utils::unix_timestamp(),
                kind: 1,
                tags: vec![],
                content: message_to_send,
            }
            .sign(&keypair);

            sender.lock().await.send(event).await;
        }
    }
}

pub async fn handle_relays(
    event: nostr_bot::Event,
    _state: nostr_bot::State<crate::MyState>,
    bot: nostr_bot::BotInfo,
) -> nostr_bot::EventNonSigned {
    let mut text = "Right now I'm connected to these relays:\n".to_string();

    let relays = bot.connected_relays().await;
    for relay in relays {
        write!(text, "{}\n", relay).unwrap();
    }

    nostr_bot::get_reply(event, text)
}

pub async fn handle_list(
    event: nostr_bot::Event,
    state: nostr_bot::State<crate::MyState>,
) -> nostr_bot::EventNonSigned {
    let follows = state.lock().await.db.lock().unwrap().get_follows();
    let mut usernames = follows.keys().collect::<Vec<_>>();
    usernames.sort();

    let mut tags = nostr_bot::tags_for_reply(event);
    let orig_tags_count = tags.len();

    let mut text = format!("Hi, I'm following {} accounts:\n", usernames.len());
    for (index, &username) in usernames.iter().enumerate() {
        let secret = follows.get(username).unwrap();
        tags.push(vec![
            "p".to_string(),
            secret.x_only_public_key().0.to_string(),
        ]);
        write!(text, "#[{}]\n", index + orig_tags_count).unwrap();
    }

    nostr_bot::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: text,
    }
}

pub async fn handle_random(
    event: nostr_bot::Event,
    state: nostr_bot::State<crate::MyState>,
) -> nostr_bot::EventNonSigned {
    let follows = state.lock().await.db.lock().unwrap().get_follows();

    if follows.is_empty() {
        return nostr_bot::get_reply(
            event,
            String::from(
                "Hi, there are no accounts. Try to add some using 'add twitter_username' command.",
            ),
        );
    }

    let index = rand::thread_rng().gen_range(0..follows.len());

    let random_username = follows.keys().collect::<Vec<_>>()[index];

    let secret = follows.get(random_username).unwrap();

    let mut tags = nostr_bot::tags_for_reply(event);
    tags.push(vec![
        "p".to_string(),
        secret.x_only_public_key().0.to_string(),
    ]);
    let mention_index = tags.len() - 1;

    debug!("Command random: returning {}", random_username);
    nostr_bot::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags,
        content: format!("Hi, random account to follow: #[{}]", mention_index),
    }
}

pub async fn handle_add(
    event: nostr_bot::Event,
    state: nostr_bot::State<crate::MyState>,
) -> nostr_bot::EventNonSigned {
    let username = event.content[5..event.content.len()]
        .to_ascii_lowercase()
        .replace('@', "");

    let db = state.lock().await.db.clone();
    let config = state.lock().await.config.clone();

    if db.lock().unwrap().contains_key(&username) {
        let keypair = simpledb::get_user_keypair(&username, db);
        let (pubkey, _parity) = keypair.x_only_public_key();
        debug!(
            "User {} already added before. Sending existing pubkey {}",
            username, pubkey
        );
        return get_handle_response(event, &pubkey.to_string());
    }

    if db.lock().unwrap().follows_count() + 1 > config.max_follows {
        return nostr_bot::get_reply(event,
            format!("Hi, sorry, couldn't add new account. I'm already running at my max capacity ({} users).", config.max_follows));
    }

    if !twitter::user_exists(&username).await {
        return nostr_bot::get_reply(
            event,
            format!("Hi, I wasn't able to find {} on Twitter :(.", username),
        );
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
        let sender = state.lock().await.sender.clone();
        let tx = state.lock().await.error_sender.clone();
        let refresh_interval_secs = config.refresh_interval_secs;
        tokio::spawn(async move {
            update_user(username, &keypair, sender, tx, refresh_interval_secs).await;
        });
    }

    get_handle_response(event, &xonly_pubkey.to_string())
}

fn get_handle_response(event: nostr_bot::Event, new_bot_pubkey: &str) -> nostr_bot::EventNonSigned {

    let mut all_tags = nostr_bot::tags_for_reply(event);
    all_tags.push(vec!["p".to_string(), new_bot_pubkey.to_string()]);
    let last_tag_position = all_tags.len() - 1;

    nostr_bot::EventNonSigned {
        created_at: utils::unix_timestamp(),
        kind: 1,
        tags: all_tags,
        content: format!(
            "Hi, tweets will be forwarded to nostr by #[{}].",
            last_tag_position
        ),
    }
}

pub async fn start_existing(
    db: simpledb::Database,
    config: &utils::Config,
    sender: nostr_bot::Sender,
    tx: ErrorSender,
) {
    for (username, keypair) in db.lock().unwrap().get_follows() {
        let tx = tx.clone();
        info!("Starting worker for username {}", username);

        {
            let refresh = config.refresh_interval_secs;
            let sender = sender.clone();
            tokio::spawn(async move {
                update_user(username, &keypair, sender, tx, refresh).await;
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
    sender: nostr_bot::Sender,
    tx: ErrorSender,
    refresh_interval_secs: u64,
) {
    // fake_worker(username, refresh_interval_secs).await;
    // return;

    let pic_url = twitter::get_pic_url(&username).await;
    let event = nostr_bot::Event::new(
        keypair,
        utils::unix_timestamp(),
        0,
        vec![],
        format!(
            r#"{{"name":"tostr_{}","about":"Tweets forwarded from https://twitter.com/{} by [tostr](https://github.com/slaninas/tostr) bot.","picture":"{}"}}"#,
            username, username, pic_url
        ),
    );

    sender.lock().await.send(event).await;

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
                    sender
                        .lock()
                        .await
                        .send(twitter::get_tweet_event(tweet).sign(keypair))
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
