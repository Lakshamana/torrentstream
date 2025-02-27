use std::env;

use serde::{de::Visitor, Deserialize, Serialize, Serializer};
use sha1::{Digest, Sha1};

// Available if you need it!
// use serde_bencode

#[derive(Serialize, Deserialize, Debug)]
struct Info {
    length: u64,
    name: String,

    #[serde(rename = "piece length")]
    piece_length: usize,
    pieces: HashList,
}

#[derive(Serialize, Deserialize, Debug)]
struct Torrent {
    announce: String,

    #[serde(rename = "created by", default)]
    created_by: String,
    info: Info,
}

#[derive(Debug)]
struct HashList(Vec<[u8; 20]>);
struct HashVisitor;

impl<'a> Visitor<'a> for HashVisitor {
    type Value = HashList;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("20-ish length byte array")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.len() % 20 != 0 {
            return Err(E::custom("Invalid length"));
        }

        Ok(HashList(
            v.chunks_exact(20)
                .map(|chunk| chunk.try_into().unwrap())
                .collect(),
        ))
    }
}

impl<'a> Deserialize<'a> for HashList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::de::Deserializer<'a>,
    {
        deserializer.deserialize_bytes(HashVisitor)
    }
}

impl Serialize for HashList {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let slice = self.0.concat();
        serializer.serialize_bytes(&slice)
    }
}

fn hash(val: &mut Vec<u8>) -> String {
    let mut hasher = Sha1::new();
    hasher.update(val);
    hex::encode(hasher.finalize())
}

#[allow(dead_code)]
fn decode_bencoded_value(encoded_value: &str) -> (serde_json::Value, &str) {
    // If encoded_value starts with a digit, it's a number
    if encoded_value.chars().next().unwrap().is_ascii_digit() {
        // string
        // Example: "5:hello" -> "hello"
        let colon_index = encoded_value.find(':').unwrap();
        let number_string = &encoded_value[..colon_index];
        let number = number_string.parse::<i64>().unwrap();
        let string = &encoded_value[(colon_index + 1)..(colon_index + 1 + number as usize)];

        (
            serde_json::Value::String(string.to_string()),
            &encoded_value[colon_index + 1 + number as usize..],
        )
    } else if encoded_value.starts_with('i') {
        // integer
        let end_idx = encoded_value.find('e').unwrap();
        let number_string = &encoded_value[1..end_idx];
        let n = number_string.parse::<i64>().unwrap();

        (
            serde_json::Value::Number(n.into()),
            &encoded_value[end_idx + 1..],
        )
    } else if encoded_value.starts_with('l') {
        // is list
        let mut inner_elm = &encoded_value[1..];
        let mut values = Vec::<serde_json::Value>::new();
        let mut skip = 0usize;

        while inner_elm != "" && !inner_elm.starts_with('e') {
            let (decoded_value, rest) = decode_bencoded_value(inner_elm);
            // println!("> {}, {}", decoded_value, rest);

            values.push(decoded_value);
            inner_elm = rest;

            if inner_elm.starts_with('e') {
                skip = 1;
                break;
            }
        }

        (serde_json::Value::Array(values), &inner_elm[skip..])
    } else if encoded_value.starts_with('d') {
        // is list
        let mut inner_elm = &encoded_value[1..];
        let mut map = serde_json::Map::new();
        let mut skip = 0usize;

        while inner_elm != "" && !inner_elm.starts_with('e') {
            let (decoded_key, rest_key) = decode_bencoded_value(inner_elm);
            let (decoded_value, rest_value) = decode_bencoded_value(rest_key);

            let end_idx = decoded_key.to_string().as_str()[1..].find('"').unwrap();
            map.insert(
                decoded_key.to_string().as_str()[1..end_idx + 1].to_string(),
                decoded_value,
            );
            inner_elm = rest_value;

            if inner_elm.starts_with('e') {
                skip = 1;
                break;
            }
        }

        (serde_json::Value::Object(map), &inner_elm[skip..])
    } else {
        panic!("Unhandled encoded value: {}", encoded_value)
    }
}

// Usage: your_bittorrent.sh decode "<encoded_value>"
fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        // Uncomment this block to pass the first stage
        let encoded_value = &args[2];
        let (decoded_value, _rest) = decode_bencoded_value(encoded_value);
        println!("{}", decoded_value);
    } else if command == "info" {
        let filename = &args[2];
        let file = std::fs::read(filename).expect("Unable to read file");

        let torrent: Torrent = serde_bencode::from_bytes(&file).unwrap();
        let mut enc_info = serde_bencode::ser::to_bytes(&torrent.info).unwrap();

        let hashes_sep = torrent
            .info
            .pieces
            .0
            .iter()
            .map(hex::encode)
            .collect::<Vec<String>>()
            .join("\n");

        // println!("{:#?}", torrent);
        println!("Tracker URL: {}", torrent.announce);
        println!("Length: {}", torrent.info.length);
        println!("Info Hash: {}", hash(&mut enc_info));
        println!("Piece Length: {}", torrent.info.piece_length);
        println!("Piece Hashes: \n{}", hashes_sep);
    } else {
        println!("unknown command: {}", args[1])
    }
}
