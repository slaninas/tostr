use log::{debug, warn};
use std::io::Write;

pub type Database = std::sync::Arc<std::sync::Mutex<SimpleDatabase>>;

pub struct SimpleDatabase {
    follows: std::collections::HashMap<String, String>,
    file: String,
}

impl SimpleDatabase {
    pub fn from_file(path: String) -> SimpleDatabase {
        let mut db = SimpleDatabase {
            follows: std::collections::HashMap::new(),
            file: path.clone(),
        };

        if !std::path::Path::new(&path).exists() {
            warn!("Database path {} doesn't exist, creating a new file", path);
            std::fs::File::create(path.clone()).expect("Failed to create file");
        }

        let content = std::fs::read_to_string(path).expect("Failed opening database file");

        for line in content.lines() {
            let split = line.split(':').collect::<Vec<_>>();
            if split.len() != 2 {
                debug!("unable to parse line: >{:?}<, skipping", split);
                continue;
            }
            let username = split[0];
            let seckey = split[1];

            match db.follows.insert(username.to_string(), seckey.to_string()) {
                Some(_) => panic!(
                    "Inconsistent database, username {} is more than once in the database",
                    username
                ),
                None => {
                    debug!(
                        "Read from file: inserting username {} into database",
                        username
                    );
                }
            }
        }

        db
    }

    pub fn insert(&mut self, username: String, seckey: String) -> Result<(), String> {
        if self.follows.contains_key(&username) {
            return Err("Key already in the database".to_string());
        }

        self.follows.insert(username.clone(), seckey.clone());
        debug!("Added {} to the database", username);

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(self.file.clone())
            .unwrap();

        writeln!(file, "{}:{}", username, seckey).unwrap();
        debug!("Wrote updated database to the file");
        Ok(())
    }

    pub fn get(&self, key: &str) -> String {
        self.follows.get(key).unwrap().to_string()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.follows.contains_key(key)
    }

    pub fn get_follows(&self) -> std::collections::HashMap<String, secp256k1::KeyPair> {
        let mut result = std::collections::HashMap::<String, secp256k1::KeyPair>::new();
        let secp = secp256k1::Secp256k1::new();
        for (username, secret) in &self.follows {
            result.insert(
                username.clone(),
                secp256k1::KeyPair::from_seckey_str(&secp, secret).unwrap(),
            );
        }
        result
    }

    pub fn follows_count(&self) -> usize {
        self.follows.len()
    }
}

pub fn get_user_keypair(username: &str, db: Database) -> secp256k1::KeyPair {
    let secp = secp256k1::Secp256k1::new();
    let existing_secret = db.lock().unwrap().get(username);
    secp256k1::KeyPair::from_seckey_str(&secp, &existing_secret).unwrap()
}
