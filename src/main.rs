mod constants;
mod pool;
mod queue;
mod types;

use anyhow::Result;
use clap::Parser;
use sha1::{Digest, Sha1};
use std::fs::File;
use std::result::Result::Ok;
use std::{char, io::Write};
use types::{Args, Command, Message, Peer, Torrent, TorrentServerRequest, TorrentServerResponse};

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

fn hash(val: &mut Vec<u8>) -> String {
    let mut hasher = Sha1::new();
    hasher.update(val);
    hex::encode(hasher.finalize())
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

fn peer_has_piece(bitfield: Message, p_index: usize) -> bool {
    let bitset = bitfield
        .payload
        .iter()
        .fold(String::from(""), |acc, &v| format!("{acc}{:b}", v));

    return bitset.chars().nth(p_index) == Some('1');
}

fn print_progress(piece_idx: usize, num_pieces: usize) {
    let progress = (piece_idx as f64 / num_pieces as f64) * 100.0;
    print!("\rDonwloading {:.2}%", progress);
    std::io::stdout().flush().unwrap();
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Download {
            torrent_path,
            output,
        } => {
            let file = std::fs::read(torrent_path).expect("Unable to read torrent file");

            let torrent: Torrent = serde_bencode::from_bytes(&file).unwrap();

            let mut peers = get_peers(&torrent).await?;
            let mut enc_info = serde_bencode::ser::to_bytes(&torrent.info).unwrap();
            let hash_info = hex::decode(hash(&mut enc_info))?;

            let num_pieces = (torrent.info.length / torrent.info.piece_length) as usize;
            let mut pieces: Vec<Vec<u8>> = Vec::new();

            println!("Downloading file...");
            for piece_idx in 0..num_pieces {
                for peer in &mut peers {
                    print_progress(piece_idx, num_pieces);

                    if let Ok(Some(bitfield)) = peer.handshake(&hash_info).await {
                        if peer_has_piece(bitfield, piece_idx) {
                            let mut piece = peer.download_piece(&torrent, piece_idx as u64).await?;
                            let hash_cmp = torrent.info.hash_by_index(piece_idx);

                            let hash = hash(&mut piece);
                            if hash != hash_cmp {
                                continue;
                            }

                            pieces.push(piece);
                            break;
                        }
                    }
                }
            }

            if pieces.is_empty() || pieces.len() != num_pieces as usize {
                anyhow::bail!("Failed to download all pieces");
            }

            let mut file = File::create(output)?;
            file.write_all(&pieces.concat())?;
        }
    };

    Ok(())
}
