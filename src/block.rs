//! Blöcke, Proof-of-Work und Merkle-Baum.

use crate::params::*;
use crate::tx::{sha256d, Hash32, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BlockHeader {
    pub version: u32,
    pub height: u64,
    pub prev_hash: Hash32,
    pub merkle_root: Hash32,
    pub timestamp: u64,
    pub target: u128,
    pub nonce: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Block {
    pub header: BlockHeader,
    pub txs: Vec<Transaction>,
}

impl BlockHeader {
    pub fn hash(&self) -> Hash32 {
        sha256d(&bincode::serialize(self).unwrap())
    }

    /// Proof-of-Work: die ersten 16 Bytes des Hashs als Zahl müssen <= Target sein
    pub fn meets_target(&self) -> bool {
        hash_value(&self.hash()) <= self.target
    }
}

pub fn hash_value(hash: &Hash32) -> u128 {
    let mut b = [0u8; 16];
    b.copy_from_slice(&hash[..16]);
    u128::from_be_bytes(b)
}

pub fn merkle_root(txids: &[Hash32]) -> Hash32 {
    if txids.is_empty() {
        return [0u8; 32];
    }
    let mut layer: Vec<Hash32> = txids.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        for pair in layer.chunks(2) {
            let mut buf = Vec::with_capacity(64);
            buf.extend_from_slice(&pair[0]);
            buf.extend_from_slice(pair.get(1).unwrap_or(&pair[0]));
            next.push(sha256d(&buf));
        }
        layer = next;
    }
    layer[0]
}

/// Der Genesis-Block ist auf allen Nodes identisch und enthält keine Coins.
/// Die ersten Coins entstehen mit Block 1 durch Mining.
pub fn genesis() -> Block {
    let mut root = [0u8; 32];
    let msg_hash = sha256d(GENESIS_MESSAGE.as_bytes());
    root.copy_from_slice(&msg_hash);
    Block {
        header: BlockHeader {
            version: 1,
            height: 0,
            prev_hash: [0u8; 32],
            merkle_root: root,
            timestamp: GENESIS_TIMESTAMP,
            target: GENESIS_TARGET,
            nonce: 0,
        },
        txs: vec![],
    }
}
