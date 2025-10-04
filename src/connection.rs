use anyhow::Result;

use std::sync::Arc;

use crate::pool::PeerPool;

pub struct ConnectionStats {
    pub connected: usize,
    pub failed: usize,
}

pub async fn validate_peers_connection(
    pool: Arc<PeerPool>,
    hash_info: &[u8],
) -> Result<ConnectionStats> {
    match pool.handshake_all(hash_info).await {
        Ok(n) => Ok(ConnectionStats {
            connected: n,
            failed: pool.len() - n,
        }),
        Err(_) => anyhow::bail!("Failed to connect to any peers"),
    }
}
