use std::{
    fmt,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use anyhow::Result;
use sha1::{Digest, Sha1};

use crate::{
    constants::BLOCK_SIZE,
    types::{Message, MessageTypes, Torrent},
    utils::{calc_piece_offset, get_piece_size},
};

pub struct Peer {
    pub address: SocketAddrV4,
    pub connection: Option<TcpStream>,
    pub id: Option<String>,
    pub bitfield: Option<Message>,
    pub is_connected: AtomicBool,
    pub stats: PeerStats,
}

impl Clone for Peer {
    fn clone(&self) -> Self {
        Self {
            address: self.address,
            connection: self.connection.as_ref().and_then(|c| c.try_clone().ok()),
            id: self.id.clone(),
            bitfield: self.bitfield.clone(),
            is_connected: AtomicBool::new(self.is_connected.load(Ordering::Acquire)),
            stats: self.stats.clone(),
        }
    }
}

impl Peer {
    pub fn from_socket(address: SocketAddrV4) -> Self {
        Self {
            address,
            connection: None,
            id: None,
            stats: PeerStats::new(),
            bitfield: None,
            is_connected: AtomicBool::new(false),
        }
    }

    pub fn new(bytes: &[u8]) -> Self {
        let ip = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
        let port = u16::from_be_bytes([bytes[4], bytes[5]]);

        Self {
            address: SocketAddrV4::new(ip, port),
            id: None,
            connection: None,
            stats: PeerStats::new(),
            bitfield: None,
            is_connected: AtomicBool::new(false),
        }
    }

    pub fn has_piece(&self, piece_idx: usize) -> bool {
        let piece_idx = piece_idx / 8;
        let bit_idx = piece_idx % 8;

        self.bitfield
            .as_ref()
            .and_then(|bitfield| bitfield.payload.get(piece_idx))
            .map_or(false, |&byte| (byte >> (7 - bit_idx)) & 1 == 1)
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

    async fn _handshake(&mut self, hash_info: &[u8]) -> Result<Option<Message>> {
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

    pub async fn handshake(&mut self, hash_info: &[u8]) -> Result<()> {
        match self._handshake(hash_info).await {
            Ok(Some(msg)) => {
                self.bitfield = Some(msg);
                self.is_connected.store(true, Ordering::Release);
                Ok(())
            }
            Err(_) | Ok(None) => {
                self.stats
                    .connection_failures
                    .fetch_add(1, Ordering::Relaxed);
                self.is_connected.store(false, Ordering::Release);
                Err(anyhow::anyhow!("Failed to connect to peer"))
            }
        }
    }

    pub fn can_accept_download(&self, max_concurrency: usize) -> bool {
        self.is_connected.load(Ordering::Acquire) && self.stats.get_load_score() < max_concurrency
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    pub fn mark_disconnected(&self) {
        self.is_connected.store(false, Ordering::Release);
        self.stats
            .connection_failures
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("Peer({}:{})", self.address.ip(), self.address.port()).as_str())
    }
}

#[derive(Debug)]
pub struct PeerStats {
    active_downloads: AtomicUsize,
    completed_pieces: AtomicUsize,
    failed_pieces: AtomicUsize,
    total_download_time_ms: AtomicU64,
    last_activity: Mutex<Instant>,
    connection_failures: AtomicUsize,
}
impl Clone for PeerStats {
    fn clone(&self) -> Self {
        Self {
            active_downloads: AtomicUsize::new(self.active_downloads.load(Ordering::Acquire)),
            completed_pieces: AtomicUsize::new(self.completed_pieces.load(Ordering::Acquire)),
            failed_pieces: AtomicUsize::new(self.failed_pieces.load(Ordering::Acquire)),
            total_download_time_ms: AtomicU64::new(
                self.total_download_time_ms.load(Ordering::Acquire),
            ),
            last_activity: Mutex::new(*self.last_activity.lock().unwrap()),
            connection_failures: AtomicUsize::new(self.connection_failures.load(Ordering::Acquire)),
        }
    }
}
impl PeerStats {
    fn new() -> Self {
        Self {
            active_downloads: AtomicUsize::new(0),
            completed_pieces: AtomicUsize::new(0),
            failed_pieces: AtomicUsize::new(0),
            total_download_time_ms: AtomicU64::new(0),
            last_activity: Mutex::new(Instant::now()),
            connection_failures: AtomicUsize::new(0),
        }
    }

    fn increment_active(&self) {
        self.active_downloads.fetch_add(1, Ordering::Release);
    }

    fn decrement_active(&self) {
        self.active_downloads.fetch_sub(1, Ordering::Release);
    }

    fn record_completion(&self, download_time_ms: u64) {
        self.completed_pieces.fetch_add(1, Ordering::Release);
        self.total_download_time_ms
            .fetch_add(download_time_ms, Ordering::Release);
    }

    fn record_failure(&self) {
        self.failed_pieces.fetch_add(1, Ordering::Release);
    }

    fn get_average_speed(&self) -> f64 {
        let total_time = self.total_download_time_ms.load(Ordering::Acquire);
        let total_pieces = self.completed_pieces.load(Ordering::Acquire);
        if total_time == 0 || total_pieces == 0 {
            0.0
        } else {
            (total_pieces as f64 * 1000.0) / (total_time as f64)
        }
    }

    pub fn get_load_score(&self) -> usize {
        self.active_downloads.load(Ordering::Acquire)
    }
}
