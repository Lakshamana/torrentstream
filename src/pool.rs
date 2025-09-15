use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use crate::{
    queue::PieceRequestQueue,
    types::{Message, Peer},
};

#[derive(Debug)]
pub struct PeerStats {
    active_downloads: AtomicUsize,
    completed_pieces: AtomicUsize,
    failed_pieces: AtomicUsize,
    total_download_time_ms: AtomicU64,
    last_activity: Mutex<Instant>,
    connection_failures: AtomicUsize,
}

pub struct PeerWithStats {
    pub peer: Peer,
    pub stats: PeerStats,
    pub bitfield: Option<Message>,
    pub is_connected: AtomicBool,
}

struct PeerPool {
    peers: Arc<Mutex<Vec<PeerWithStats>>>,
    max_concurrent_per_peer: usize,
    total_active_downloads: Arc<AtomicUsize>,
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

    fn get_load_score(&self) -> usize {
        self.active_downloads.load(Ordering::Acquire)
    }
}

impl PeerWithStats {
    fn new(peer: Peer) -> Self {
        Self {
            peer,
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

    fn can_accept_download(&self, max_concurrency: usize) -> bool {
        self.is_connected.load(Ordering::Acquire) && self.stats.get_load_score() < max_concurrency
    }
}

impl PeerPool {
    fn new(peers: Vec<Peer>, max_concurrent_per_peer: usize) -> Self {
        Self {
            peers: Arc::new(Mutex::new(
                peers.into_iter().map(PeerWithStats::new).collect(),
            )),
            max_concurrent_per_peer,
            total_active_downloads: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn is_peer_available(&self, peer_idx: usize) -> bool {
        match self.peers.lock() {
            Ok(peers) => {
                if let Some(peer) = peers.get(peer_idx) {
                    peer.can_accept_download(self.max_concurrent_per_peer)
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    fn find_best_peer_for_piece(
        &self,
        piece_idx: usize,
        exclude_peer_ids: &[String],
    ) -> Option<usize> {
        self.peers.lock().map_or(None, |peers| {
            peers
                .iter()
                .enumerate()
                .filter(|(_, peer)| {
                    peer.has_piece(piece_idx)
                        && peer.can_accept_download(self.max_concurrent_per_peer)
                        && peer
                            .peer
                            .id
                            .as_ref()
                            .map_or(true, |peer_id| !exclude_peer_ids.contains(peer_id))
                })
                .min_by_key(|(_, peer)| peer.stats.get_load_score())
                .map(|(peer_idx, _)| peer_idx)
        })
    }

    async fn get_pool_stats(&self) -> (usize, usize, usize) {
        (
            self.total_active_downloads.load(Ordering::Acquire),
            self.peers.lock().unwrap().len(),
            self.max_concurrent_per_peer,
        )
    }
}
