# tostr
/ˈtəʊstə(r)/

**T**witter to n**ostr**.
Bot that forwards tweets to [nostr](https://github.com/nostr-protocol/nostr).

You can interact with it using 'add twitter_username' or 'random' commands.


## How it works
It uses [twint](https://github.com/minamotorin/twint.git) to get the tweets, making nostr events from them which are signed using
[secp256k1](https://crates.io/crates/secp256k1) and send to relays using [tokio_tungstenite](https://crates.io/crates/tokio-tungstenite).

## How to run
```
git clone https://github.com/slaninas/tostr/ && cd tostr
# Add secret to config file, choose refresh interval, relay and set limit for number of accounts
./build_and_run.sh --clearnet|--tor
```
Now the bot should be running and waiting for mentions. You can reply to its message with 'add twitter_username' to add new account or with 'random' to get a random user.
It relays only new tweets that were posted after you launched it.

## Tor
In case `--tor` is used connections to both relay and Twitter *should* be going through tor. But if you need full anonymity please **check yourself there are no leaks**.


## Known limitations/issues
- in `update_user` function, `since` value may not correspond to the previous `until` value (seems it breaks shortly after a new tweet is found), this may lead to tweets being forwarded twice or not at all
- twint is a Twitter scraper that currently works but who knows for how long
- Doesn't work for retweets by users you follow (twint has `--retweets` option but it's extremely slow 1.5 vs 30 s for some accounts)
- ~~There are multiple processes spawned for each account check and relaying, twint also takes some time to process so it's slow,
I tested it with 40 accounts and it took almost a minute to check if there were any new tweets for them.~~ (under 4.5 s now, only twint process is spawned now)
- ~~But it shows replies by people you follow (is that good or bad?)~~
- ~~Tweets containing ' or " are not relayed~~

## TODOs
- [ ] Error handling
- [ ] Don't send `set_metadata` again after reconnect
- [ ] Cleanup
- [ ] Set timeout for connection
- [ ] Proxy support
- [x] ~~Use existing websocket crate instead of spawning websocat process~~
- [x] ~~Find solution better than spawning nostril process~~
- [x] ~~Parallelization, async?~~
- [x] ~~Follow Twitters redirects and send original url to nostr~~
- [x] ~~Add proper logging~~
- [x] ~~Check timestamps that are used for twint's --since option, if the new tweets check takes too long some tweets may get ignored during the next check~~
- [x] ~~Read "hello" message from a config instead of using hardcoded one and post it when bot starts~~
