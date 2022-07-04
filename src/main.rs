use log::{debug, info};
use std::io::Write;

const DATE_FORMAT_STR: &'static str = "%Y-%m-%d %H:%M:%S";

#[tokio::main]
async fn main() {
    let start = std::time::Instant::now();
    env_logger::Builder::from_default_env()
        .format(move |buf, rec| {
            let t = start.elapsed().as_secs_f32();
            writeln!(buf, "{:.03} [{}] - {}", t, rec.level(), rec.args())
        })
        .init();

    let config_path = std::path::PathBuf::from("config");
    let config = parse_config(&config_path);
    debug!("{:?}", config);

    let time: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();

    let mut last_update =
        std::collections::HashMap::<String, chrono::DateTime<chrono::offset::Local>>::new();

    let mut handles = vec![];

    for username in config.follow {
        // last_update.insert(username.to_string(), time);
        let secret = config.secret.clone();
        let relays = config.relays.clone();
        debug!("Spawning update user task for {}", username);
        handles.push(tokio::spawn(async move {
            update_user(username, secret, relays, config.refresh_interval_secs).await;
        }));
    }

    for handle in handles {
        tokio::join!(handle);
    }
}

#[derive(Debug)]
struct Tweet {
    date: String,
    username: String,
    tweet: String,
    link: String,
}

struct Config {
    secret: String,
    refresh_interval_secs: u64,
    relays: Vec<String>,
    follow: Vec<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("Config")
            .field("secret", &"***")
            .field("refresh_interval_secs", &self.refresh_interval_secs)
            .field("relays", &self.relays)
            .field("follow", &self.follow)
            .finish()
    }
}

async fn update_user(
    username: String,
    secret: String,
    relays: Vec<String>,
    refresh_interval_secs: u64,
) {
    let mut since: chrono::DateTime<chrono::offset::Local> = std::time::SystemTime::now().into();
    loop {
        debug!("Going to sleep for {} s", refresh_interval_secs);
        tokio::time::sleep(std::time::Duration::from_secs(refresh_interval_secs)).await;

        let new_tweets = get_new_tweets(&username, since);
        since = std::time::SystemTime::now().into();

        // twint returns newest tweets first, reverse the Vec here so that tweets are send to relays
        // in order they were published. Still the created_at field can easily be the same so in the
        // end it depends on how the relays handle it
        for tweet in new_tweets.iter().rev() {
            send_tweet(tweet, &secret, &relays).await;
        }
        // break;
    }
}

async fn send_tweet(tweet: &Tweet, secret: &String, relays: &Vec<String>) {
    let formatted = format!(
        "[@{}@twitter.com]({}): {}",
        tweet.username, tweet.link, tweet.tweet
    );

    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let event = tostr::Event::new(secret.clone(), formatted, timestamp);

    debug!("new event: {}", event.format());

    for relay in relays {
        debug!("Sending >{}< to {}", event.format(), relay);
        event.send(relay).await;
    }
}

fn get_new_tweets(username: &String, since: chrono::DateTime<chrono::offset::Local>) -> Vec<Tweet> {
    debug!("Checking new tweets from {}", username);
    let workfile = format!("{}_workfile.csv", username);

    let cmd = format!(
        "twint -u {} --since \"{}\" --csv -o {}",
        username,
        since.format(DATE_FORMAT_STR),
        workfile
    );
    debug!("Running >{}<", cmd);
    let mut output = std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();

    let mut new_tweets = vec![];
    match std::fs::read_to_string(workfile.clone()) {
        Ok(content) => {
            std::fs::remove_file(workfile).unwrap();

            let mut csv = content.lines().collect::<Vec<_>>();

            let header = csv[0].split("\t").collect::<Vec<_>>();

            for i in 1..csv.len() {
                let line = csv[i].split("\t").collect::<Vec<_>>();
                new_tweets.push(Tweet {
                    date: format!("{} {} {}", line[3], line[4], line[5]),
                    username: line[7].to_string(),
                    tweet: line[10].to_string(),
                    link: line[20].to_string(),
                });
            }

            info!("Found {} new tweets from {}", new_tweets.len(), username);
        }
        Err(_) => {
            info!("No new tweets from {} found", username);
        }
    }

    new_tweets
}

fn parse_config(path: &std::path::Path) -> Config {
    let get_value = |line: String| line.split('=').collect::<Vec<_>>()[1].to_string();

    let content = std::fs::read_to_string(path).expect("Config reading failed.");

    let mut secret = String::new();
    let mut refresh_interval_secs = 0;
    let mut relays = vec![];
    let mut follow = vec![];

    for line in content.lines() {
        let line = line.to_string();

        if line.starts_with("secret") {
            secret = get_value(line);
        } else if line.starts_with("refresh_interval_secs") {
            refresh_interval_secs = get_value(line)
                .parse::<u64>()
                .expect("Failed to parse the refresh interval.");
        } else if line.starts_with("add_relay") {
            relays.push(get_value(line))
        } else if line.starts_with("add_follow") {
            follow.push(get_value(line))
        }
    }

    assert!(secret.len() > 0);
    assert!(refresh_interval_secs > 0);
    assert!(relays.len() > 0);
    assert!(follow.len() > 0);

    Config {
        secret,
        refresh_interval_secs,
        relays,
        follow,
    }
}

