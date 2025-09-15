use sha1::{Digest, Sha1};

use crate::types::Torrent;

pub fn calc_piece_offset(piece_idx: usize, piece_length: u64) -> u64 {
    piece_idx as u64 * piece_length
}

pub fn get_piece_size(torrent: &Torrent, piece_idx: usize) -> u64 {
    let total_pieces =
        (torrent.info.length + torrent.info.piece_length - 1) / torrent.info.piece_length;

    assert!(
        piece_idx < total_pieces as usize,
        "Piece index {} out of range [0, {})",
        piece_idx,
        total_pieces
    );

    if piece_idx as u64 == total_pieces - 1 {
        let remainder = torrent.info.length % torrent.info.piece_length;
        if remainder == 0 {
            torrent.info.piece_length
        } else {
            remainder
        }
    } else {
        torrent.info.piece_length
    }
}

pub fn verify_piece_hash(data: &[u8], expected_hash: &str) -> bool {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hex::encode(hasher.finalize()) == expected_hash
}
