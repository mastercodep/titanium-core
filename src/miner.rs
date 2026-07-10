//! Proof-of-Work-Miner mit mehreren Threads.

use crate::block::{hash_value, merkle_root, Block, BlockHeader};
use crate::node::Node;
use crate::params::*;
use crate::tx::{sha256d, Hash32, Transaction};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn spawn_miner(node: Arc<Node>) {
    std::thread::spawn(move || loop {
        if !node.mining.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        let Some(block) = build_template(&node) else {
            node.mining.store(false, Ordering::Relaxed);
            node.log("Mining gestoppt: kein Wallet vorhanden.");
            continue;
        };
        let threads = node.mining_threads.load(Ordering::Relaxed).max(1);
        let stop = AtomicBool::new(false);
        let winner: Mutex<Option<u64>> = Mutex::new(None);
        let started = Instant::now();
        let tip_at_start = block.header.prev_hash;

        std::thread::scope(|s| {
            for t in 0..threads {
                let node = &node;
                let block = &block;
                let stop = &stop;
                let winner = &winner;
                s.spawn(move || {
                    let mut buf = bincode::serialize(&block.header).unwrap();
                    let nonce_off = buf.len() - 8;
                    let mut nonce = t as u64;
                    let step = threads as u64;
                    let mut count: u64 = 0;
                    loop {
                        buf[nonce_off..].copy_from_slice(&nonce.to_le_bytes());
                        let h = sha256d(&buf);
                        count += 1;
                        if hash_value(&h) <= block.header.target {
                            *winner.lock().unwrap() = Some(nonce);
                            stop.store(true, Ordering::Relaxed);
                            break;
                        }
                        if count % 20_000 == 0 {
                            node.hash_counter.fetch_add(20_000, Ordering::Relaxed);
                            if stop.load(Ordering::Relaxed)
                                || !node.mining.load(Ordering::Relaxed)
                            {
                                break;
                            }
                            // Template auffrischen: neue Zeit/Transaktionen oder neuer Tip
                            if started.elapsed() > Duration::from_secs(10)
                                || node.chain.lock().unwrap().tip_hash() != tip_at_start
                            {
                                break;
                            }
                        }
                        nonce = nonce.wrapping_add(step);
                    }
                });
            }
        });

        let found = *winner.lock().unwrap();
        if let Some(nonce) = found {
            let mut b = block;
            b.header.nonce = nonce;
            // submit_block validiert erneut; falls das Netzwerk schneller war, wird abgelehnt
            let _ = node.submit_block(b);
        }
    });
}

fn build_template(node: &Arc<Node>) -> Option<Block> {
    let addr = {
        let wallet = node.wallet.lock().unwrap();
        wallet.as_ref()?.address()
    };
    let chain = node.chain.lock().unwrap();
    let height = chain.height() + 1;
    let target = chain.expected_target(height);
    let prev_hash = chain.tip_hash();

    // Gültige Mempool-Transaktionen einsammeln
    let mut selected: Vec<Transaction> = Vec::new();
    let mut spent: HashSet<crate::tx::OutPoint> = HashSet::new();
    let mut fees: u64 = 0;
    for tx in chain.mempool.values() {
        if selected.len() >= 2000 {
            break;
        }
        if let Ok(fee) = chain.validate_tx(tx, &spent) {
            for input in &tx.inputs {
                spent.insert(input.prev);
            }
            fees += fee;
            selected.push(tx.clone());
        }
    }

    let reward = block_reward(height) + fees;
    let mut txs = vec![Transaction::coinbase(height, addr, reward.max(1))];
    txs.extend(selected);
    let txids: Vec<Hash32> = txs.iter().map(|t| t.txid()).collect();

    Some(Block {
        header: BlockHeader {
            version: 1,
            height,
            prev_hash,
            merkle_root: merkle_root(&txids),
            timestamp: now_ts(),
            target,
            nonce: 0,
        },
        txs,
    })
}
