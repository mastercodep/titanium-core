//! Persistenz: Blockchain, Wallet und Peer-Liste auf der Festplatte.

use crate::block::Block;
use std::fs;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(base).join("TitaniumCore");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn save_chain(dir: &PathBuf, blocks: &[Block]) {
    if let Ok(data) = bincode::serialize(blocks) {
        let tmp = dir.join("chain.dat.tmp");
        let path = dir.join("chain.dat");
        if fs::write(&tmp, data).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

pub fn load_chain(dir: &PathBuf) -> Option<Vec<Block>> {
    let data = fs::read(dir.join("chain.dat")).ok()?;
    bincode::deserialize(&data).ok()
}

pub fn save_wallet_secret(dir: &PathBuf, secret_hex: &str) {
    let _ = fs::write(dir.join("wallet.dat"), secret_hex);
}

pub fn load_wallet_secret(dir: &PathBuf) -> Option<String> {
    fs::read_to_string(dir.join("wallet.dat")).ok()
}

pub fn save_peers(dir: &PathBuf, peers: &[String]) {
    let _ = fs::write(dir.join("peers.txt"), peers.join("\n"));
}

pub fn load_peers(dir: &PathBuf) -> Vec<String> {
    fs::read_to_string(dir.join("peers.txt"))
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}
