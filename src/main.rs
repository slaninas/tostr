use log::{info, debug};
use std::io::Write;

const DATE_FORMAT_STR: &'static str = "%Y-%m-%d %H:%M:%S";

fn main() {
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

    for username in &config.follow {
        last_update.insert(username.to_string(), time);
    }

    loop {
        info!("Going to sleep for {} s", config.refresh_interval_secs);
        std::thread::sleep(std::time::Duration::from_secs(config.refresh_interval_secs));

        info!("Update check for all accounts starting");
        for username in &config.follow {
            let new_tweets = get_new_tweets(username, last_update.get(username).unwrap().clone());

            last_update.insert(username.clone(), std::time::SystemTime::now().into());

            for tweet in new_tweets {
                send_tweet(tweet, &config.secret, &config.relays);
            }
        }
        info!("Update check for all accounts is done");
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

fn send_tweet(tweet: Tweet, secret: &String, relays: &Vec<String>) {
    let formatted = format!(
        "[@{}@twitter.com]({}): {}",
        tweet.username, tweet.link, tweet.tweet
    );

    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(format!(
            "/nostril/nostril --envelope --sec {} --content \"{}\"",
            secret, formatted
        ))
        .stdout(std::process::Stdio::piped())
        .output()
        .unwrap();

    let event = String::from_utf8(output.stdout).unwrap();

    for relay in relays {
        debug!("Sending >{}< to {}", event, relay);

        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!("echo '{}' | websocat {}", event, relay))
            .output()
            .unwrap();
    }
}

fn get_new_tweets(username: &String, since: chrono::DateTime<chrono::offset::Local>) -> Vec<Tweet> {
    debug!("Checking new tweets from {}", username);
    let workfile = "workfile.csv";

    let cmd = format!(
        "twint -u {} --since \"{}\" --csv -o {} --retweets",
        username,
        since.format(DATE_FORMAT_STR),
        workfile
    );
    // println!("cmd >{}<", cmd);
    let mut output = std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();

    let mut new_tweets = vec![];
    match std::fs::read_to_string(workfile) {
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

            debug!("Found {} new tweets from {}", new_tweets.len(), username);
        }
        Err(e) => {
            debug!("No new tweets from {} found", username);
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

