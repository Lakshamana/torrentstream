use std::{
    collections::BinaryHeap,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::{
    constants::{DEFAULT_RANDOM_RATIO, DEFAULT_RAREST_FIRST_RATIO, DEFAULT_SEQUENTIAL_RATIO},
    pool::PeerWithStats,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceRequest {
    pub piece_idx: usize,
    pub priority: u8,
    pub retry_count: usize,
    pub rarity_score: u8,
    pub last_attempt: Option<Instant>,
    pub last_peer_id: Option<String>,
}

pub struct PieceRequestQueue {
    pub queue: Arc<Mutex<BinaryHeap<PieceRequest>>>,
    pub strategy: PiecePriorityStrategy,
}

impl PieceRequestQueue {
    pub fn new(
        strategy: PiecePriorityStrategy,
        total_pieces: usize,
        peers: &[PeerWithStats],
    ) -> Self {
        let requests = (0..total_pieces).map(|piece_idx| {
            let frequency = peers
                .iter()
                .filter(|peer| peer.has_piece(piece_idx))
                .count();

            let rarity_score = 255 - (frequency * 255 / peers.len()) as u8; // 0-255

            PieceRequest::new(piece_idx, rarity_score, &strategy, total_pieces)
        });

        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::from_iter(requests))),
            strategy,
        }
    }

    pub fn pop_next_piece(&self) -> Option<PieceRequest> {
        match self.queue.lock() {
            Ok(mut queue) => queue.pop().map(|mut request| {
                request.last_attempt = Some(Instant::now());
                request
            }),
            Err(_) => None,
        }
    }

    pub fn requeue_failed_piece(&self, mut piece_req: PieceRequest) {
        if let Ok(mut queue) = self.queue.lock() {
            piece_req.increment_retry();
            queue.push(piece_req)
        }
    }

    pub fn requeue_with_delay(&self, piece_req: PieceRequest, delay: Duration) {
        thread::sleep(delay);
        self.requeue_failed_piece(piece_req);
    }

    pub fn is_empty(&self) -> bool {
        match self.queue.lock() {
            Ok(queue) => queue.is_empty(),
            Err(_) => true,
        }
    }

    pub fn remaining_pieces_len(&self) -> usize {
        match self.queue.lock() {
            Ok(queue) => queue.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PiecePriorityStrategy {
    RarestFirst,
    Sequential,
    Random,
    Hybrid {
        rarest_first_ratio: f32,
        sequential_ratio: f32,
        random_ratio: f32,
    },
}

impl Ord for PieceRequest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.priority != other.priority {
            return self.priority.cmp(&other.priority);
        }

        if self.rarity_score != other.rarity_score {
            return self.rarity_score.cmp(&other.rarity_score);
        }

        self.piece_idx.cmp(&other.piece_idx)
    }
}

impl PartialOrd for PieceRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PieceRequest {
    fn new(
        piece_idx: usize,
        rarity_score: u8,
        strategy: &PiecePriorityStrategy,
        total_pieces: usize,
    ) -> Self {
        let priority = match strategy {
            PiecePriorityStrategy::RarestFirst => rarity_score,
            PiecePriorityStrategy::Sequential => {
                calc_priority_seq(piece_idx, rarity_score, total_pieces)
            }
            PiecePriorityStrategy::Random => calc_priority_random(),
            PiecePriorityStrategy::Hybrid { .. } => calculate_hybrid_priority(
                piece_idx,
                rarity_score,
                total_pieces,
                DEFAULT_RAREST_FIRST_RATIO,
                DEFAULT_SEQUENTIAL_RATIO,
                DEFAULT_RANDOM_RATIO,
            ),
        };

        Self {
            piece_idx,
            priority,
            retry_count: 0,
            rarity_score,
            last_attempt: None,
            last_peer_id: None,
        }
    }

    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    fn should_retry(&self, max_retries: usize, min_retry_delay: Duration) -> bool {
        self.retry_count < max_retries
            && self.last_attempt.map_or(true, |last_attempt| {
                last_attempt.elapsed() >= min_retry_delay
            })
    }
}

fn calc_priority_seq(piece_idx: usize, _rarity_score: u8, total_pieces: usize) -> u8 {
    let normalized = ((total_pieces - piece_idx) as f32 / total_pieces as f32) * 255.0;
    normalized as u8
}

fn calc_priority_random() -> u8 {
    rand::random_range(0..=255)
}

fn calculate_hybrid_priority(
    piece_idx: usize,
    rarity_score: u8,
    total_pieces: usize,
    rarest_first_ratio: f32,
    sequential_ratio: f32,
    random_ratio: f32,
) -> u8 {
    let rarest_component = rarity_score as f32 * rarest_first_ratio;

    let sequential_base = ((total_pieces - piece_idx) as f32 / total_pieces as f32) * 255.0;
    let sequential_component = sequential_base * sequential_ratio;

    let random_component = rand::random_range(0..=255) as f32 * random_ratio;

    let total_priority = rarest_component + sequential_component + random_component;
    total_priority.min(255.0) as u8
}
