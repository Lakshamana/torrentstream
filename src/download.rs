use std::{
    fs::File,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;

use crate::{
    connection::validate_peers_connection,
    constants::MAX_RETRIES,
    peer::Peer,
    pool::PeerPool,
    queue::{PiecePriorityStrategy, PieceRequestQueue},
    types::{DownloadState, Torrent},
    utils::{calc_total_pieces, hash},
};

fn is_network_related_error(e: &anyhow::Error) -> bool {
    let e_str = e.to_string();

    e_str.contains("Connection reset by peer")
        || e_str.contains("Connection timed out")
        || e_str.contains("Connection refused")
        || e_str.contains("Broken pipe")
        || e_str.contains("No route to host")
        || e_str.contains("Network is unreachable")
}

async fn download_worker(
    worker_id: usize,
    pool: Arc<PeerPool>,
    download_state: Arc<DownloadState>,
    piece_queue: Arc<PieceRequestQueue>,
    torrent: Torrent,
    output_file: Arc<Mutex<File>>,
) -> Result<()> {
    loop {
        if download_state.is_complete() {
            break;
        }

        let piece_req = match piece_queue.pop_next_piece() {
            Some(req) => req,
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };

        let except_list = piece_req
            .last_peer_id
            .as_ref()
            .map_or(vec![], |id| vec![id]);
        let peer_idx = match pool.find_best_peer_for_piece(piece_req.piece_idx, &except_list) {
            Some(idx) => idx,
            None => {
                let piece_req_arc = Arc::new(piece_req);
                let queue_clone = piece_queue.clone();
                tokio::spawn(async move {
                    queue_clone
                        .requeue_with_delay(piece_req_arc, Duration::from_secs(2))
                        .await
                });
                continue;
            }
        };

        let mut peer = pool.get_peer_by_index(peer_idx).unwrap();
        let peer_id = peer.id.as_ref().unwrap().clone();

        download_state.mark_piece_in_progress(
            piece_req.piece_idx,
            &peer_id,
            format!("worker-{}", worker_id),
        );

        let download_result = peer
            .download_piece(&torrent, piece_req.piece_idx, output_file.clone())
            .await;

        match download_result {
            Ok(()) => download_state.mark_piece_completed(piece_req.piece_idx),
            Err(e) => {
                let piece_idx = piece_req.piece_idx;

                // Distinguish between network errors and other errors
                let is_network_error = is_network_related_error(&e);

                if is_network_error {
                    // Mark peer as disconnected and increment failure counter
                    if let Some(peer_with_stats) = pool.get_peer_by_index(peer_idx) {
                        peer_with_stats.mark_disconnected();
                        println!(
                            "⚠️ Peer {} disconnected during piece {} download",
                            peer_id, piece_idx
                        );
                    }
                }

                download_state.mark_piece_failed(piece_idx, e.to_string());
                let piece_req_arc = Arc::new(piece_req);
                let queue_clone = piece_queue.clone();
                if piece_req_arc.should_retry(MAX_RETRIES, Duration::from_secs(1)) {
                    tokio::spawn(async move {
                        queue_clone
                            .requeue_with_delay(piece_req_arc, Duration::from_secs(2))
                            .await
                    });
                } else {
                    eprintln!("⚠️ Piece {} failed permanently: {}", piece_idx, e);
                }
            }
        }
    }

    Ok(())
}

async fn report_progress(
    download_state: Arc<DownloadState>,
    update_interval: Duration,
) -> Result<()> {
    loop {
        let progress = download_state.get_progress_snapshot();

        let completed_percent =
            (progress.completed_pieces as f64 / progress.total_pieces as f64) * 100.0;
        let bytes_percent =
            (progress.bytes_downloaded as f64 / progress.total_bytes as f64) * 100.0;

        print!(
            "\rDownloaded: {:.1}% ({}/{} pieces, {:.1}% of {} bytes) | In progress: {}, Failed: {}",
            completed_percent,
            progress.completed_pieces,
            progress.total_pieces,
            bytes_percent,
            progress.total_bytes,
            progress.in_progress_pieces,
            progress.failed_pieces,
        );

        if download_state.is_complete() {
            println!("\rDownload complete!");
            break;
        }

        tokio::time::sleep(update_interval).await;
    }

    Ok(())
}

pub async fn download(
    torrent: Torrent,
    peers: Vec<Peer>,
    output_path: &PathBuf,
    max_concurrent_workers: usize,
) -> Result<()> {
    let output_file = {
        let file = File::create(output_path)?;
        file.set_len(torrent.info.length)?;
        Arc::new(Mutex::new(file))
    };

    let total_pieces = calc_total_pieces(&torrent);
    let download_state = Arc::new(DownloadState::new(total_pieces, torrent.info.length));
    let pool = Arc::new(PeerPool::new(peers, max_concurrent_workers));

    let strategy = PiecePriorityStrategy::Hybrid {
        rarest_first_ratio: 0.7,
        sequential_ratio: 0.2,
        random_ratio: 0.1,
    };
    let piece_queue = Arc::new(PieceRequestQueue::new(
        strategy,
        total_pieces,
        &pool.peers.lock().unwrap(),
    ));

    let mut enc_info = serde_bencode::ser::to_bytes(&torrent.info).unwrap();
    let hash_info = hex::decode(hash(&mut enc_info))?;

    // Validate peer connections before starting workers
    let stats = validate_peers_connection(Arc::clone(&pool), &hash_info).await?;
    println!(
        "Connected to {}/{} peers",
        stats.connected,
        stats.connected + stats.failed
    );

    assert!(stats.connected > 0, "No peers connected");

    let mut workers: Vec<_> = (0..max_concurrent_workers)
        .map(|worker_id| {
            let pool_clone = Arc::clone(&pool);
            let download_state_clone = Arc::clone(&download_state);
            let piece_queue_clone = Arc::clone(&piece_queue);
            let output_file_clone = Arc::clone(&output_file);

            tokio::spawn(download_worker(
                worker_id,
                pool_clone,
                download_state_clone,
                piece_queue_clone,
                torrent.clone(),
                output_file_clone,
            ))
        })
        .collect();

    let progress_reporter = tokio::spawn(report_progress(
        Arc::clone(&download_state),
        Duration::from_secs(1),
    ));

    workers.push(progress_reporter);
    futures::future::join_all(workers).await;

    Ok(())
}
