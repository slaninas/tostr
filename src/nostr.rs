use secp256k1::Secp256k1;

use log::debug;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub msg_type: String,
    pub subscription_id: String,
    pub content: Event,
}

pub struct EventNonSigned {
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

impl EventNonSigned {
    pub fn sign(self, keypair: &secp256k1::KeyPair) -> Event {
        Event::new(keypair, self.content, self.created_at, self.tags)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

impl Event {
    pub fn new(
        keypair: &secp256k1::KeyPair,
        content: String,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) -> Self {
        let secp = Secp256k1::new();

        let (pubkey, _parity) = keypair.x_only_public_key();

        let mut formatted_tags = Self::format_tags(&tags);
        formatted_tags.retain(|c| !c.is_whitespace());

        let msg = format!(
            r#"[0,"{}",{},1,[{}],"{}"]"#,
            pubkey, created_at, formatted_tags, content
        );
        debug!("commitment '{}'\n", msg);
        let id =
            secp256k1::Message::from_hashed_data::<secp256k1::hashes::sha256::Hash>(msg.as_bytes());

        let signature = secp.sign_schnorr(&id, &keypair);

        Event {
            id: id.to_string(),
            pubkey: pubkey.to_string(),
            created_at,
            kind: 1,
            content,
            sig: signature.to_string(),
            tags,
        }
    }

    pub fn format(&self) -> String {
        format!(
            r#"["EVENT",{{"id": "{}", "pubkey": "{}", "created_at": {}, "kind": {}, "tags": [{}], "content": "{}", "sig": "{}"}}]"#,
            self.id,
            self.pubkey,
            self.created_at,
            self.kind,
            Self::format_tags(&self.tags),
            self.content,
            self.sig
        )
    }
    pub fn print(&self) {
        println!("{}", self.format());
    }

    fn format_tags(tags: &Vec<Vec<String>>) -> String {
        let mut formatted = String::new();

        for i in 0..tags.len() {
            let tag = &tags[i];
            formatted.push_str(&format!(r#"["{}"]"#, tag.join(r#"", ""#)));
            if i + 1 < tags.len() {
                formatted.push_str(", ");
            }
        }
        formatted
    }
}

pub fn get_tags_for_reply(event: Event) -> Vec<Vec<String>> {
    let mut e_tags = vec![];
    let mut p_tags = vec![];
    for t in event.tags {
        if t[0] == "e" {
            e_tags.push(t);
        } else if t[0] == "p" {
            p_tags.push(t);
        }
    }

    debug!("Got e_tags: {:?}", e_tags);
    debug!("Got p_tags: {:?}", p_tags);

    p_tags.push(vec!["p".to_string(), event.pubkey]);

    let mut all_tags = p_tags;
    if e_tags.len() > 0 {
        all_tags.push(e_tags[0].clone());
        all_tags.push(vec!["e".to_string(), event.id]);
    }

    all_tags
}
