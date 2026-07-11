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
    std::thread::spawn(move || {
        // GPU einmalig initialisieren (inkl. Selbsttest gegen den CPU-Pfad)
        let gpu = crate::gpu::GpuMiner::init();
        match &gpu {
            Some(g) => {
                node.gpu_available.store(true, Ordering::Relaxed);
                *node.gpu_name.lock().unwrap() = g.device_name.clone();
                node.log(format!(
                    "GPU erkannt: {} — GPU-Mining verfügbar.",
                    g.device_name
                ));
            }
            None => {
                node.gpu_available.store(false, Ordering::Relaxed);
                node.log("Keine nutzbare GPU/OpenCL gefunden — Mining läuft auf der CPU.");
            }
        }

        loop {
            if !node.mining.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            let Some(block) = build_template(&node) else {
                node.mining.store(false, Ordering::Relaxed);
                node.log("Mining gestoppt: kein Wallet vorhanden.");
                continue;
            };
            let use_gpu = node.use_gpu.load(Ordering::Relaxed) && gpu.is_some();
            let winner = if use_gpu {
                mine_gpu(&node, &block, gpu.as_ref().unwrap())
            } else {
                mine_cpu(&node, block.clone())
            };
            if let Some(nonce) = winner {
                let mut b = block;
                b.header.nonce = nonce;
                let _ = node.submit_block(b);
            }
        }
    });
}

/// GPU-Mining eines Block-Templates. Gibt den Gewinner-Nonce zurück oder None,
/// wenn abgebrochen wird (Template veraltet, Mining aus, GPU abgewählt).
fn mine_gpu(node: &Arc<Node>, block: &Block, gpu: &crate::gpu::GpuMiner) -> Option<u64> {
    let bytes = bincode::serialize(&block.header).unwrap();
    if bytes.len() != 108 {
        // Unerwartetes Format — sicher auf CPU ausweichen
        return mine_cpu(node, block.clone());
    }
    let mut header = [0u8; 108];
    header.copy_from_slice(&bytes);
    let target = block.header.target;
    let tip_at_start = block.header.prev_hash;
    let started = Instant::now();
    let mut base: u64 = 0;
    loop {
        if let Some(nonce) = gpu.search(&header, target, base) {
            // Auf der CPU gegenprüfen, bevor der Block eingereicht wird
            let mut b = block.clone();
            b.header.nonce = nonce;
            if b.header.meets_target() {
                node.hash_counter.fetch_add(gpu.batch, Ordering::Relaxed);
                return Some(nonce);
            }
        }
        node.hash_counter.fetch_add(gpu.batch, Ordering::Relaxed);
        base = base.wrapping_add(gpu.batch);
        if !node.mining.load(Ordering::Relaxed) || !node.use_gpu.load(Ordering::Relaxed) {
            return None;
        }
        if started.elapsed() > Duration::from_secs(10)
            || node.chain.lock().unwrap().tip_hash() != tip_at_start
        {
            return None;
        }
    }
}

/// CPU-Mining mit mehreren Threads. Gibt den Gewinner-Nonce zurück oder None.
fn mine_cpu(node: &Arc<Node>, block: Block) -> Option<u64> {
    {
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
        found
    }
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
