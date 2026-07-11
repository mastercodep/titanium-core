//! Zentraler Node-Zustand, geteilt zwischen GUI, Miner und Netzwerk.

use crate::chain::Chain;
use crate::network::Msg;
use crate::storage;
use crate::tx::Hash32;
use crate::wallet::Wallet;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

pub struct PeerHandle {
    pub addr: String,
    pub sender: Sender<Msg>,
    pub height: u64,
}

pub struct Node {
    pub chain: Mutex<Chain>,
    pub wallet: Mutex<Option<Wallet>>,
    pub peers: Mutex<HashMap<u64, PeerHandle>>,
    pub known_peers: Mutex<HashSet<String>>,
    pub mining: AtomicBool,
    pub mining_threads: AtomicUsize,
    /// GPU-Mining nutzen, sofern eine GPU verfügbar ist
    pub use_gpu: AtomicBool,
    /// wird vom Miner gesetzt, sobald eine GPU erkannt wurde
    pub gpu_available: AtomicBool,
    pub gpu_name: Mutex<String>,
    pub hash_counter: AtomicU64,
    pub blocks_mined: AtomicU64,
    pub listen_port: u16,
    /// Zufällige ID dieses Nodes — erkennt versehentliche Verbindungen mit sich selbst
    pub node_id: u64,
    pub next_peer_id: AtomicU64,
    pub data_dir: PathBuf,
    pub log: Mutex<Vec<String>>,
}

impl Node {
    pub fn new(listen_port: u16) -> Self {
        let dir = storage::data_dir();
        let chain = match storage::load_chain(&dir).and_then(|b| Chain::from_blocks(b).ok()) {
            Some(c) => c,
            None => Chain::new(),
        };
        let wallet = storage::load_wallet_secret(&dir).and_then(|s| Wallet::from_secret_hex(&s));
        let known: HashSet<String> = storage::load_peers(&dir).into_iter().collect();
        Node {
            chain: Mutex::new(chain),
            wallet: Mutex::new(wallet),
            peers: Mutex::new(HashMap::new()),
            known_peers: Mutex::new(known),
            mining: AtomicBool::new(false),
            mining_threads: AtomicUsize::new((std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                / 2)
            .max(1)),
            use_gpu: AtomicBool::new(true),
            gpu_available: AtomicBool::new(false),
            gpu_name: Mutex::new(String::new()),
            hash_counter: AtomicU64::new(0),
            blocks_mined: AtomicU64::new(0),
            listen_port,
            node_id: rand::random(),
            next_peer_id: AtomicU64::new(1),
            data_dir: dir,
            log: Mutex::new(vec!["Node gestartet.".into()]),
        }
    }

    pub fn log(&self, msg: impl Into<String>) {
        let mut log = self.log.lock().unwrap();
        log.push(msg.into());
        if log.len() > 200 {
            let excess = log.len() - 200;
            log.drain(..excess);
        }
    }

    pub fn save_chain(&self) {
        let chain = self.chain.lock().unwrap();
        storage::save_chain(&self.data_dir, &chain.blocks);
    }

    pub fn save_peers(&self) {
        let known = self.known_peers.lock().unwrap();
        let list: Vec<String> = known.iter().cloned().collect();
        storage::save_peers(&self.data_dir, &list);
    }

    /// Nachricht an alle verbundenen Peers (optional einen ausnehmen)
    pub fn broadcast(&self, msg: &Msg, except: Option<u64>) {
        let peers = self.peers.lock().unwrap();
        for (id, p) in peers.iter() {
            if Some(*id) != except {
                let _ = p.sender.send(msg.clone());
            }
        }
    }

    /// Eigene Transaktion einreichen: Mempool + Broadcast
    pub fn submit_tx(&self, tx: crate::tx::Transaction) -> Result<Hash32, String> {
        let txid = {
            let mut chain = self.chain.lock().unwrap();
            chain.add_to_mempool(tx.clone())?
        };
        self.broadcast(&Msg::NewTx(tx), None);
        self.log(format!("Transaktion {} gesendet.", short_hash(&txid)));
        Ok(txid)
    }

    /// Selbst geminten Block einreichen: anhängen + speichern + Broadcast
    pub fn submit_block(&self, block: crate::block::Block) -> Result<(), String> {
        {
            let mut chain = self.chain.lock().unwrap();
            chain.add_block(block.clone())?;
        }
        self.save_chain();
        self.blocks_mined.fetch_add(1, Ordering::Relaxed);
        self.broadcast(&Msg::NewBlock(block.clone()), None);
        self.log(format!(
            "Block {} gemint! Belohnung: {} TCORE",
            block.header.height,
            crate::params::format_tcore(block.txs[0].outputs.iter().map(|o| o.amount).sum())
        ));
        Ok(())
    }

    pub fn peer_count(&self) -> usize {
        self.peers.lock().unwrap().len()
    }
}

pub fn short_hash(h: &Hash32) -> String {
    let s = hex::encode(h);
    format!("{}…{}", &s[..8], &s[56..])
}
