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
        let mut cut = 0 as usize;

        while inner_elm != "" && !inner_elm.starts_with('e') {
            let (decoded_value, rest) = decode_bencoded_value(inner_elm);
            // println!("> {}, {}", decoded_value, rest);

            values.push(decoded_value);
            inner_elm = rest;

            if inner_elm.starts_with('e') {
                cut = 1;
                break;
            }
        }

        (serde_json::Value::Array(values), &inner_elm[cut..])
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
        println!("{}", decoded_value.to_string());
    } else {
        println!("unknown command: {}", args[1])
    }
}
