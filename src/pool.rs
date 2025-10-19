use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::Result;
use tokio::time::timeout;

use crate::peer::Peer;

pub struct PeerPool {
    pub peers: Arc<Mutex<Vec<Peer>>>,
    max_concurrent_per_peer: usize,
    total_active_downloads: Arc<AtomicUsize>,
}
impl PeerPool {
    pub fn new(peers: Vec<Peer>, max_concurrent_per_peer: usize) -> Self {
        Self {
            peers: Arc::new(Mutex::new(peers)),
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

    pub fn get_peer_by_index(&self, peer_idx: usize) -> Option<Peer> {
        self.peers
            .lock()
            .map_or(None, |peers| peers.get(peer_idx).cloned())
    }

    pub fn find_best_peer_for_piece(
        &self,
        piece_idx: usize,
        exclude_peer_ids: &[&String],
    ) -> Option<usize> {
        self.peers.lock().map_or(None, |peers| {
            let p = peers
                .iter()
                .enumerate()
                .filter(|(_, peer)| {
                    peer.has_piece(piece_idx)
                        && peer.can_accept_download(self.max_concurrent_per_peer)
                        && peer
                            .id
                            .as_ref()
                            .map_or(true, |peer_id| !exclude_peer_ids.contains(&peer_id))
                })
                .min_by_key(|(_, peer)| peer.stats.get_load_score());

            if p.is_some() {
                let (idx, p) = p.unwrap();
                p.stats.increment_active();
                return Some(idx);
            }

            None
        })
    }

    async fn get_pool_stats(&self) -> (usize, usize, usize) {
        (
            self.total_active_downloads.load(Ordering::Acquire),
            self.peers.lock().unwrap().len(),
            self.max_concurrent_per_peer,
        )
    }

    pub async fn handshake_all(&self, hash_info: &[u8]) -> Result<usize> {
        let mut peers = match self.peers.lock() {
            Ok(peers) => peers,
            Err(poisoned) => poisoned.into_inner(),
        };

        let n_connected = if !peers.is_empty() {
            let tasks: Vec<_> = peers
                .iter_mut()
                .map(|p| {
                    let hash_info = hash_info.to_vec();
                    timeout(Duration::from_secs(5), async move {
                        p.handshake(&hash_info).await
                    })
                })
                .collect();

            futures::future::join_all(tasks)
                .await
                .into_iter()
                .filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok())
                .count()
        } else {
            0
        };

        Ok(n_connected)
    }

    pub fn len(&self) -> usize {
        self.peers.lock().unwrap().len()
    }
}
