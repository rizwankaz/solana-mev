use crate::fetcher::BlockFetcher;
use crate::types::{FetchedBlock, FetcherError, Result};
use futures::stream::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
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
        let (tx, rx) = mpsc::channel(100);
        
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
                    },
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
        let (tx, rx) = mpsc::channel(100);
        
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
            
            loop {
                // fetch current block
                match fetcher.fetch_block(current_slot).await {
                    Ok(block) => {
                        if tx.send((current_slot, Ok(block))).await.is_err() {
                            break;
                        }
                        current_slot += 1;
                    },
                    Err(FetcherError::BlockNotAvailable { .. }) => {
                        // block not produced yet
                        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                        
                        // check if behind
                        if let Ok(latest) = fetcher.get_current_slot().await {
                            if latest > current_slot {
                                current_slot = latest;
                            }
                        }
                    },
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

impl Stream for BlockStream {
    type Item = (u64, Result<FetchedBlock>);
    
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}