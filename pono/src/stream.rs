use crate::fetcher::BlockFetcher;
use crate::types::{FetchedBlock, FetcherError, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// stream of blocks starting from given slot
pub struct BlockStream {
    receiver: mpsc::Receiver<(u64, Result<FetchedBlock>)>,
    _handle: tokio::task::JoinHandle<()>,
}

impl BlockStream {
    /// create new block stream starting from given slot
    pub fn new(fetcher: Arc<BlockFetcher>, start_slot: u64) -> Self {
        let (tx, rx) = mpsc::channel(10);

        let handle = tokio::spawn(async move {
            let mut current_slot = start_slot;

            loop {
                match fetcher.fetch_block(current_slot).await {
                    Ok(block) => {
                        if tx.send((current_slot, Ok(block))).await.is_err() {
                            info!("block stream receiver dropped, stopping");
                            break;
                        }
                        current_slot += 1;
                    }
                    Err(e) => {
                        if tx.send((current_slot, Err(e))).await.is_err() {
                            break;
                        }
                        current_slot += 1;
                    }
                }
            }
        });

        Self {
            receiver: rx,
            _handle: handle,
        }
    }

    /// create a stream that follows chain tip
    pub fn follow_tip(fetcher: Arc<BlockFetcher>) -> Self {
        let (tx, rx) = mpsc::channel(50);

        let handle = tokio::spawn(async move {
            // get starting slot
            let mut current_slot = match fetcher.get_current_slot().await {
                Ok(slot) => slot,
                Err(e) => {
                    warn!("failed to get current slot: {:?}", e);
                    return;
                }
            };

            info!("following chain tip starting from slot {}", current_slot);

            let mut consecutive_unavailable = 0u32;
            let mut last_latest_check = std::time::Instant::now();

            loop {
                // Check latest slot every 2 seconds to stay current
                if last_latest_check.elapsed().as_secs() >= 2 {
                    if let Ok(latest_slot) = fetcher.get_current_slot().await {
                        let lag = latest_slot.saturating_sub(current_slot);
                        if lag > 20 {
                            debug!(
                                "Catching up: jumping from slot {} to {} ({} slots behind)",
                                current_slot,
                                latest_slot.saturating_sub(5),
                                lag
                            );
                            current_slot = latest_slot.saturating_sub(5);
                            consecutive_unavailable = 0;
                        }
                    }
                    last_latest_check = std::time::Instant::now();
                }

                // fetch current block
                match fetcher.fetch_block(current_slot).await {
                    Ok(block) => {
                        if tx.send((current_slot, Ok(block))).await.is_err() {
                            break;
                        }
                        current_slot += 1;
                        consecutive_unavailable = 0;
                    }
                    Err(FetcherError::BlockNotAvailable { .. }) => {
                        consecutive_unavailable += 1;

                        if consecutive_unavailable == 1 {
                            // First time unavailable - slot might be skipped or not yet confirmed
                            // Wait a short time for confirmation
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                        } else if consecutive_unavailable >= 3 {
                            // Slot was likely skipped (no leader, network issue, etc.)
                            debug!("Skipping slot {} (not produced after {} attempts)", current_slot, consecutive_unavailable);
                            current_slot += 1;
                            consecutive_unavailable = 0;
                        } else {
                            // Wait a bit longer before next retry
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    }
                    Err(e) => {
                        warn!("error fetching slot {}: {:?}", current_slot, e);
                        if tx.send((current_slot, Err(e))).await.is_err() {
                            break;
                        }
                        current_slot += 1;
                        consecutive_unavailable = 0;
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        });

        Self {
            receiver: rx,
            _handle: handle,
        }
    }

    /// receive next block
    pub async fn next(&mut self) -> Option<(u64, Result<FetchedBlock>)> {
        self.receiver.recv().await
    }
}
