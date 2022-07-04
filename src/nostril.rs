include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub struct KeyPair {
    pub secret: Vec<u8>,
    pub public: Vec<u8>,

}
pub fn generate_keypair() -> KeyPair {

    let mut secret = vec![0u8; 33];
    let mut public = vec![0u8; 33];

    unsafe {
        nostril_get_keys(secret.as_mut_ptr(), public.as_mut_ptr());
    }

    secret.truncate(32);
    public.truncate(32);
    KeyPair {
        secret,
        public,
    }
}

pub fn key_to_hex(key: &Vec<u8>) -> String {
    let mut result = String::new();
    for byte in key {
        let first_nibble = byte / 16;
        let second_nibble = byte % 16;
        result.push(get_hex_digit(first_nibble));
        result.push(get_hex_digit(second_nibble));
    }
    result
}

pub fn create_event(content: String, mut pubkey: Vec<u8>, mut secret: Vec<u8>) {
    let mut buffer = vec![0u8; 1024];

    let mut content_i8 = vec![];

    for b in content.as_bytes() {
        content_i8.push(*b as i8);
    }

    let mut pubkey_zero = pubkey.clone();
    pubkey_zero.push(0);

    let mut secret_zero = secret.clone();
    secret_zero.push(0);

    // TODO: immutable
    unsafe {
        nostril_create_event(pubkey_zero.as_mut_ptr(), secret_zero.as_mut_ptr(), content_i8.as_mut_ptr(), buffer.as_mut_ptr());
    }

}

pub fn call_main() {
    let x = 3;
    let mut c0 = String::from("nostril");
    let mut c = String::from("--content");
    let mut c2 = String::from("blah");
    let mut carray: [*const std::os::raw::c_char; 3] = [c0.as_ptr() as _, c.as_ptr() as  _, c2.as_ptr() as _];
    unsafe {
        fake_main(x, carray.as_mut_ptr());
    }
}

fn get_hex_digit(c: u8) -> char {
    match c {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        15 => 'f',
        _ => panic!("Out of range."),
    }
}
