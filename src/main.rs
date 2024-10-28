use serde_json;
use std::env;

// Available if you need it!
// use serde_bencode

#[allow(dead_code)]
fn decode_bencoded_value(encoded_value: &str) -> (serde_json::Value, &str) {
    // If encoded_value starts with a digit, it's a number
    if encoded_value.chars().next().unwrap().is_digit(10) {
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
        let mut skip = 0 as usize;

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
        let mut skip = 0 as usize;

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
    } else {
        println!("unknown command: {}", args[1])
    }
}
