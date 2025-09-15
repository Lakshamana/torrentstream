use crate::{
    constants::BLOCK_SIZE,
    utils::{calc_piece_offset, get_piece_size},
};
use anyhow::Result;
use clap::{Parser, Subcommand};
use core::fmt;
use sha1::{Digest, Sha1};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    mem,
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    ops::Index,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use serde::{de::Visitor, Deserialize, Serialize, Serializer};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
#[clap(rename_all = "snake_case")]
pub enum Command {
    Download {
        #[arg(short, long)]
        output: PathBuf,
        torrent_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageTypes {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

pub struct Message {
    pub prefix: i32,
    pub id: MessageTypes,
    pub payload: Vec<u8>,
}

impl TryFrom<u8> for MessageTypes {
    type Error = String;

    fn try_from(value: u8) -> Result<MessageTypes, Self::Error> {
        match value {
            0 => Ok(MessageTypes::Choke),
            1 => Ok(MessageTypes::Unchoke),
            2 => Ok(MessageTypes::Interested),
            3 => Ok(MessageTypes::NotInterested),
            4 => Ok(MessageTypes::Have),
            5 => Ok(MessageTypes::Bitfield),
            6 => Ok(MessageTypes::Request),
            7 => Ok(MessageTypes::Piece),
            8 => Ok(MessageTypes::Cancel),
            _ => Err(format!("Unknown message type {value}")),
        }
    }
}

impl MessageTypes {
    pub fn as_byte(&self) -> u8 {
        self.clone() as u8
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("Message[id={:?}, payload={:?}]", self.id, self.payload).as_str())
    }
}

impl Message {
    pub fn new(id: MessageTypes, payload: &[u8]) -> Self {
        Self {
            id,
            prefix: payload.len() as i32 + 1,
            payload: payload.to_owned(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        [
            &self.prefix.to_be_bytes(),
            [self.id.as_byte(); 1].as_slice(),
            &self.payload,
        ]
        .concat()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    pub length: u64,
    pub name: String,

    #[serde(rename = "piece length")]
    pub piece_length: u64,
    pub pieces: HashList,
}

impl Info {
    pub fn get_hashes(&self) -> Vec<String> {
        self.pieces
            .0
            .iter()
            .map(hex::encode)
            .collect::<Vec<String>>()
    }

    pub fn hash_by_index(&self, index: usize) -> String {
        self.get_hashes()[index].clone()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Torrent {
    pub announce: String,

    #[serde(rename = "created by", default)]
    pub created_by: String,
    pub info: Info,
}

#[derive(Debug)]
pub struct HashList(pub Vec<[u8; 20]>);
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

#[derive(Debug, Deserialize, Serialize)]
pub struct TorrentServerRequest {
    pub info_hash: String,
    pub peer_id: String,
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub compact: u8,
}

impl TorrentServerRequest {
    pub fn query_string(self) -> String {
        let json = serde_json::to_value::<Self>(self).unwrap();

        let qs = json
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<String>>()
            .join("&")
            .replace('"', "");

        format!("?{}", qs)
    }
}

#[derive(Debug, Deserialize)]
pub struct TorrentServerResponse {
    #[serde(with = "serde_bytes")]
    pub peers: Vec<u8>,
}

#[derive(Debug)]
pub struct Peer {
    pub address: SocketAddrV4,
    pub connection: Option<TcpStream>,
    pub id: Option<String>,
}

impl Peer {
    pub fn from_socket(address: SocketAddrV4) -> Self {
        Self {
            address,
            connection: None,
            id: None,
        }
    }

    pub fn new(bytes: &[u8]) -> Self {
        let ip = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
        let port = u16::from_be_bytes([bytes[4], bytes[5]]);

        Peer {
            address: SocketAddrV4::new(ip, port),
            id: None,
            connection: None,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Vec<Peer> {
        bytes.chunks_exact(6).map(Peer::new).collect()
    }

    async fn r_peer_msg(&mut self) -> Result<Option<Message>> {
        let mut m_size = [0u8; 4];
        let mut m_type = [0u8; 1];

        if let Some(connection) = self.connection.as_mut() {
            connection.read_exact(&mut m_size)?;
            connection.read_exact(&mut m_type)?;

            let length = u32::from_be_bytes(m_size);
            let id = u8::from_be_bytes(m_type);

            let id_result = MessageTypes::try_from(id);
            if id_result.is_err() {
                return Ok(None);
            }

            let mut payload_buf = vec![0u8; length as usize - 1];
            connection.read_exact(&mut payload_buf)?;

            return Ok(Some(Message::new(id_result.unwrap(), &payload_buf)));
        }

        Err(anyhow::format_err!("Connection not set"))
    }

    pub async fn handshake(&mut self, hash_info: &[u8]) -> Result<Option<Message>> {
        let mut stream = TcpStream::connect(self.address)?;
        let protocol_str = "BitTorrent protocol";
        let peer_id = "lakshamana_torclient".to_string();

        let mut rbuf = [0u8; 68];
        let wbuf = [
            &[19u8],
            protocol_str.as_bytes(),
            &[0u8; 8],
            hash_info,
            peer_id.as_bytes(),
        ]
        .concat();

        stream.write_all(&wbuf)?;
        stream.read_exact(&mut rbuf)?;

        self.connection = Some(stream);
        self.id = Some(hex::encode(&rbuf[48..]));
        println!(">> Handshaking peer {}", self);

        self.r_peer_msg().await
    }

    pub async fn send_msg(&mut self, message: &Message) -> Result<Message> {
        if let Some(connection) = self.connection.as_mut() {
            connection.write_all(&message.to_bytes()).unwrap();
            let resp_msg = self.r_peer_msg().await?;

            return Ok(resp_msg.expect("Response message not found"));
        }

        Err(anyhow::format_err!("Connection not set"))
    }

    async fn download_chunk(&mut self, index: i32, begin: i32, length: i32) -> Result<Message> {
        let payload = [
            index.to_be_bytes(),
            begin.to_be_bytes(),
            length.to_be_bytes(),
        ]
        .concat();

        let request = Message::new(MessageTypes::Request, &payload);
        let response = self.send_msg(&request).await?;

        Ok(response)
    }

    pub async fn download_piece(
        &mut self,
        torrent: &Torrent,
        piece_idx: usize,
        output_file: Arc<Mutex<File>>,
    ) -> Result<()> {
        let unchoke_msg = self
            .send_msg(&Message::new(MessageTypes::Interested, &[]))
            .await?;

        if unchoke_msg.id != MessageTypes::Unchoke {
            panic!("Expected unchoke message");
        }

        let piece_length = torrent.info.piece_length;
        let piece_offset = calc_piece_offset(piece_idx, piece_length);
        let p_size = get_piece_size(torrent, piece_idx);

        let mut remainder = p_size as i32;
        let mut begin = 0i32;
        let mut hasher = Sha1::new();

        loop {
            print!(
                "\rLoading chunk {}%...",
                ((p_size as i32 - remainder) * 100 / p_size as i32)
            );
            let b_size = BLOCK_SIZE.min(remainder);

            let chunk = self.download_chunk(piece_idx as i32, begin, b_size).await?;
            let c_payload = &chunk.payload[8..];
            hasher.update(c_payload);

            match output_file.lock() {
                Ok(mut file) => {
                    file.seek(SeekFrom::Start(piece_offset + begin as u64))?;
                    file.write_all(c_payload)?;
                }
                Err(poisoned) => {
                    let mut file = poisoned.into_inner();
                    file.seek(SeekFrom::Start(piece_offset + begin as u64))?;
                    file.write_all(c_payload)?;
                }
            }

            remainder -= b_size;
            begin += b_size;

            if remainder == 0 {
                print!(
                    "\rLoading chunk {}%...",
                    ((p_size as i32 - remainder) * 100 / p_size as i32)
                );
                break;
            }
        }

        let actual_hash = hex::encode(hasher.finalize());
        let expected_hash = torrent.info.hash_by_index(piece_idx);
        if actual_hash != expected_hash {
            return Err(anyhow::format_err!("Piece hash mismatch"));
        }

        Ok(())
    }
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("Peer({}:{})", self.address.ip(), self.address.port()).as_str())
    }
}

#[derive(Clone, Debug)]
pub enum PieceStatus {
    Pending,
    InProgress {
        peer_id: String,
        started_at: Instant,
    },
    Completed {
        written_to_disk: bool,
    },
    Failed {
        retry_count: usize,
        last_error: String,
    },
}

#[derive(Debug)]
pub struct DownloadProgress {
    completed_pieces: usize,
    in_progress_pieces: usize,
    failed_pieces: usize,
    total_pieces: usize,
    bytes_downloaded: u64,
    total_bytes: u64,
}

pub struct DownloadState {
    piece_status: Arc<Mutex<Vec<PieceStatus>>>,
    completed_pieces: Arc<AtomicUsize>,
    in_progress_pieces: Arc<AtomicUsize>,
    failed_pieces: Arc<AtomicUsize>,
    total_pieces: usize,
    bytes_downloaded: Arc<AtomicU64>,
    total_bytes: u64,
}

impl DownloadState {
    pub fn new(total_pieces: usize, total_bytes: u64) -> Self {
        Self {
            piece_status: Arc::new(Mutex::new(vec![PieceStatus::Pending; total_pieces])),
            completed_pieces: Arc::new(AtomicUsize::new(0)),
            in_progress_pieces: Arc::new(AtomicUsize::new(0)),
            failed_pieces: Arc::new(AtomicUsize::new(0)),
            total_pieces,
            bytes_downloaded: Arc::new(AtomicU64::new(0)),
            total_bytes,
        }
    }

    fn get_piece_status(&self, piece_idx: usize) -> PieceStatus {
        assert!(
            piece_idx < self.total_pieces && piece_idx >= 0,
            "Piece index out of bounds {}",
            piece_idx
        );

        match self.piece_status.lock() {
            Ok(guard) => guard[piece_idx].clone(),
            Err(poisoned) => {
                println!("Poisoned piece status");
                poisoned.into_inner()[piece_idx].clone()
            }
        }
    }

    fn set_piece_status(&self, piece_idx: usize, status: PieceStatus) {
        assert!(
            piece_idx < self.total_pieces && piece_idx >= 0,
            "Piece index out of bounds {}",
            piece_idx
        );

        match self.piece_status.lock() {
            Ok(mut guard) => {
                let old_status = mem::replace(&mut guard[piece_idx], status);

                if let PieceStatus::InProgress { .. } = old_status {
                    self.in_progress_pieces.fetch_sub(1, Ordering::Relaxed);
                }

                match guard[piece_idx] {
                    PieceStatus::InProgress { .. } => {
                        self.in_progress_pieces.fetch_add(1, Ordering::Relaxed);
                    }
                    PieceStatus::Failed { .. } => {
                        self.failed_pieces.fetch_add(1, Ordering::Relaxed);
                    }
                    PieceStatus::Completed { .. } => {
                        self.completed_pieces.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            Err(poisoned) => {
                println!("Poisoned piece status");
                poisoned.into_inner()[piece_idx] = status;
            }
        }
    }

    pub fn mark_piece_in_progress(&self, piece_idx: usize, peer_id: String) {
        if let Ok(_) = self.piece_status.lock() {
            let status = PieceStatus::InProgress {
                peer_id,
                started_at: Instant::now(),
            };
            self.set_piece_status(piece_idx, status);
        }
    }

    pub fn mark_piece_completed(&self, piece_idx: usize) {
        if let Ok(_) = self.piece_status.lock() {
            let status = PieceStatus::Completed {
                written_to_disk: true,
            };
            self.set_piece_status(piece_idx, status);
            self.completed_pieces.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn mark_piece_failed(&self, piece_idx: usize, error: String) {
        if let Ok(guard) = self.piece_status.lock() {
            let retry_count = match guard[piece_idx] {
                PieceStatus::Failed { retry_count, .. } => retry_count + 1,
                _ => 1,
            };

            let status = PieceStatus::Failed {
                retry_count,
                last_error: error,
            };
            self.set_piece_status(piece_idx, status);
            self.failed_pieces.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn reset_piece_to_pending(&self, piece_idx: usize) {
        if let Ok(guard) = self.piece_status.lock() {
            self.set_piece_status(piece_idx, PieceStatus::Pending);
        }
    }

    pub fn get_progress_snapshot(&self) -> DownloadProgress {
        DownloadProgress {
            completed_pieces: self.completed_pieces.load(Ordering::Relaxed),
            in_progress_pieces: self.in_progress_pieces.load(Ordering::Relaxed),
            failed_pieces: self.failed_pieces.load(Ordering::Relaxed),
            total_pieces: self.total_pieces,
            bytes_downloaded: self.bytes_downloaded.load(Ordering::Relaxed),
            total_bytes: self.total_bytes,
        }
    }

    pub fn is_complete(&self) -> bool {
        // TODO: check whether Ordering::Relaxed is safe
        self.completed_pieces.load(Ordering::Relaxed) == self.total_pieces
    }
}
