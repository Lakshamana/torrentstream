mod connection;
mod constants;
mod download;
mod peer;
mod pool;
mod queue;
mod types;
mod utils;

use anyhow::Result;
use clap::Parser;
use std::fs::File;
use std::io::Write;
use std::result::Result::Ok;
use types::{Args, Command, Torrent, TorrentServerRequest, TorrentServerResponse};

use crate::{peer::Peer, utils::hash};

async fn get_peers(torrent: &Torrent) -> Result<Vec<Peer>> {
    let hash = hash(&mut serde_bencode::ser::to_bytes(&torrent.info).unwrap());
    let request_body = TorrentServerRequest {
        info_hash: encode_url(hash.as_str()),
        peer_id: "lakshamana_torclient".to_string(),
        port: 6881,
        uploaded: 0,
        downloaded: 0,
        left: torrent.info.length,
        compact: 1,
    };

    let url = format!("{}{}", torrent.announce, request_body.query_string());
    let response = reqwest::get(url).await?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let response_bytes = response.bytes().await?;
            let bencoded: TorrentServerResponse = serde_bencode::from_bytes(&response_bytes)?;

            let peers = Peer::from_bytes(&bencoded.peers);

            Ok(peers)
        }
        _ => {
            println!("Error getting peers: {:?}", response.status());

            anyhow::bail!("Failed to get peers");
        }
    }
}

fn encode_url(val: &str) -> String {
    val.as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            let num = hex::decode(chunk).unwrap()[0];
            if num.is_ascii_alphanumeric() {
                char::from(num).to_string()
            } else {
                format!("%{:02x}", num)
            }
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Download {
            torrent_path,
            output,
            workers,
        } => {
            let file = std::fs::read(torrent_path).expect("Unable to read torrent file");

            let torrent: Torrent = serde_bencode::from_bytes(&file).unwrap();

            let peers = get_peers(&torrent).await?;

            let num_pieces = (torrent.info.length / torrent.info.piece_length) as usize;
            let pieces: Vec<Vec<u8>> = Vec::new();

            println!("Downloading file...");
            download::download(torrent, peers, &output, workers).await?;

            if pieces.is_empty() || pieces.len() != num_pieces as usize {
                anyhow::bail!("Failed to download all pieces");
            }

            let mut file = File::create(output)?;
            file.write_all(&pieces.concat())?;
        }
    };

    Ok(())
}
