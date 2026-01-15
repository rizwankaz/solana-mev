use crate::fetcher::BlockFetcher;
use crate::types::{FetchedBlock, FetcherError, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

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
        let (tx, rx) = mpsc::channel(10);

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

            let mut blocks_processed = 0u64;

            loop {
                // Every 50 blocks, check if we're falling behind
                if blocks_processed % 50 == 0 && blocks_processed > 0 {
                    if let Ok(latest_slot) = fetcher.get_current_slot().await {
                        let lag = latest_slot.saturating_sub(current_slot);
                        if lag > 50 {
                            // We're more than 50 slots behind, skip ahead
                            info!(
                                "Catching up: skipping from slot {} to {} ({} slots behind)",
                                current_slot,
                                latest_slot.saturating_sub(10),
                                lag
                            );
                            current_slot = latest_slot.saturating_sub(10); // Leave small buffer
                        }
                    }
                }

                // fetch current block
                match fetcher.fetch_block(current_slot).await {
                    Ok(block) => {
                        if tx.send((current_slot, Ok(block))).await.is_err() {
                            break;
                        }
                        current_slot += 1;
                        blocks_processed += 1;
                    }
                    Err(FetcherError::BlockNotAvailable { .. }) => {
                        // block not produced yet, wait and retry
                        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                        // Don't skip slots - continue sequential processing
                    }
                    Err(e) => {
                        warn!("error fetching slot {}: {:?}", current_slot, e);
                        if tx.send((current_slot, Err(e))).await.is_err() {
                            break;
                        }
                        current_slot += 1;
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
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
