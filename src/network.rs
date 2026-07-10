//! Dezentrales P2P-Netzwerk: TCP-Gossip mit Blocksynchronisation und Peer-Austausch.

use crate::block::Block;
use crate::node::{Node, PeerHandle};
use crate::params::*;
use crate::tx::Transaction;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

const MAX_MSG_SIZE: u32 = 64 * 1024 * 1024;
const MAX_BLOCKS_PER_MSG: usize = 500;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Msg {
    Hello {
        version: u32,
        height: u64,
        listen_port: u16,
        node_id: u64,
    },
    GetBlocks {
        from: u64,
    },
    Blocks(Vec<Block>),
    NewBlock(Block),
    NewTx(Transaction),
    GetPeers,
    Peers(Vec<String>),
}

fn write_msg(stream: &mut TcpStream, msg: &Msg) -> std::io::Result<()> {
    let data = bincode::serialize(msg).map_err(|e| std::io::Error::other(e.to_string()))?;
    stream.write_all(&(data.len() as u32).to_le_bytes())?;
    stream.write_all(&data)?;
    Ok(())
}

fn read_msg(stream: &mut TcpStream) -> std::io::Result<Msg> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_MSG_SIZE {
        return Err(std::io::Error::other("Nachricht zu groß"));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;
    bincode::deserialize(&buf).map_err(|e| std::io::Error::other(e.to_string()))
}

/// Trägt die fest eingebauten Seed-Nodes ein und lädt regelmäßig
/// zusätzliche Peer-Listen aus dem Internet (falls URLs konfiguriert sind).
/// Dadurch verbindet sich jede frische Installation vollautomatisch mit dem Netzwerk.
pub fn spawn_seed_fetch(node: Arc<Node>) {
    std::thread::spawn(move || loop {
        {
            let mut known = node.known_peers.lock().unwrap();
            for seed in SEED_NODES {
                known.insert(seed.to_string());
            }
        }
        for url in PEER_LIST_URLS {
            if let Ok(resp) = ureq::get(url).timeout(Duration::from_secs(10)).call() {
                if let Ok(body) = resp.into_string() {
                    let mut known = node.known_peers.lock().unwrap();
                    for line in body.lines().take(200) {
                        let addr = line.trim().to_string();
                        if addr.parse::<std::net::SocketAddr>().is_ok() {
                            known.insert(addr);
                        }
                    }
                }
            }
        }
        node.save_peers();
        std::thread::sleep(Duration::from_secs(600));
    });
}

/// Startet den TCP-Listener für eingehende Peer-Verbindungen.
pub fn spawn_listener(node: Arc<Node>) {
    let port = node.listen_port;
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => {
                node.log(format!("Netzwerk lauscht auf Port {port}."));
                l
            }
            Err(e) => {
                node.log(format!("Port {port} konnte nicht geöffnet werden: {e}"));
                return;
            }
        };
        for stream in listener.incoming().flatten() {
            let node = node.clone();
            std::thread::spawn(move || handle_peer(node, stream, None));
        }
    });
}

/// Versucht regelmäßig, Verbindungen zu bekannten Peers aufzubauen.
pub fn spawn_connector(node: Arc<Node>) {
    std::thread::spawn(move || loop {
        let targets: Vec<String> = {
            let known = node.known_peers.lock().unwrap();
            let connected: std::collections::HashSet<String> = node
                .peers
                .lock()
                .unwrap()
                .values()
                .map(|p| p.addr.clone())
                .collect();
            known
                .iter()
                .filter(|a| !connected.contains(*a))
                .take(8)
                .cloned()
                .collect()
        };
        for addr in targets {
            let node = node.clone();
            std::thread::spawn(move || {
                if let Ok(stream) =
                    TcpStream::connect_timeout_str(&addr, Duration::from_secs(5))
                {
                    handle_peer(node, stream, Some(addr));
                }
            });
        }
        std::thread::sleep(Duration::from_secs(15));
    });
}

trait ConnectStr {
    fn connect_timeout_str(addr: &str, timeout: Duration) -> std::io::Result<TcpStream>;
}

impl ConnectStr for TcpStream {
    fn connect_timeout_str(addr: &str, timeout: Duration) -> std::io::Result<TcpStream> {
        use std::net::ToSocketAddrs;
        let sock_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::other("Adresse nicht auflösbar"))?;
        TcpStream::connect_timeout(&sock_addr, timeout)
    }
}

/// Verbindung zu einem Peer: Handshake, Sync, Gossip.
fn handle_peer(node: Arc<Node>, stream: TcpStream, dialed_addr: Option<String>) {
    let peer_ip = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    let id = node.next_peer_id.fetch_add(1, Ordering::Relaxed);
    let (tx_out, rx_out) = mpsc::channel::<Msg>();

    // Writer-Thread besitzt eine Kopie des Streams
    let mut write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let writer = std::thread::spawn(move || {
        for msg in rx_out {
            if write_msg(&mut write_stream, &msg).is_err() {
                break;
            }
        }
    });

    // Handshake senden
    let my_height = node.chain.lock().unwrap().height();
    let _ = tx_out.send(Msg::Hello {
        version: PROTOCOL_VERSION,
        height: my_height,
        listen_port: node.listen_port,
        node_id: node.node_id,
    });
    let _ = tx_out.send(Msg::GetPeers);

    let display_addr = dialed_addr.clone().unwrap_or_else(|| peer_ip.clone());
    {
        let mut peers = node.peers.lock().unwrap();
        peers.insert(
            id,
            PeerHandle {
                addr: display_addr.clone(),
                sender: tx_out.clone(),
                height: 0,
            },
        );
    }
    node.log(format!("Peer verbunden: {display_addr}"));

    let mut read_stream = stream;
    let _ = read_stream.set_read_timeout(Some(Duration::from_secs(300)));
    loop {
        let msg = match read_msg(&mut read_stream) {
            Ok(m) => m,
            Err(_) => break,
        };
        match msg {
            Msg::Hello {
                version,
                height,
                listen_port,
                node_id,
            } => {
                if version != PROTOCOL_VERSION {
                    break;
                }
                // Verbindung mit sich selbst erkannt -> Adresse vergessen und trennen
                if node_id == node.node_id {
                    if let Some(addr) = &dialed_addr {
                        node.known_peers.lock().unwrap().remove(addr);
                    }
                    break;
                }
                {
                    let mut peers = node.peers.lock().unwrap();
                    if let Some(p) = peers.get_mut(&id) {
                        p.height = height;
                    }
                }
                // Adresse des Peers für spätere Verbindungen merken
                if !peer_ip.is_empty() {
                    let addr = format!("{peer_ip}:{listen_port}");
                    node.known_peers.lock().unwrap().insert(addr);
                    node.save_peers();
                }
                let my_height = node.chain.lock().unwrap().height();
                if height > my_height {
                    let _ = tx_out.send(Msg::GetBlocks {
                        from: my_height + 1,
                    });
                }
            }
            Msg::GetBlocks { from } => {
                let chain = node.chain.lock().unwrap();
                let start = from.max(1) as usize;
                if start < chain.blocks.len() {
                    let end = (start + MAX_BLOCKS_PER_MSG).min(chain.blocks.len());
                    let _ = tx_out.send(Msg::Blocks(chain.blocks[start..end].to_vec()));
                }
            }
            Msg::Blocks(blocks) => {
                if blocks.is_empty() {
                    continue;
                }
                let mut appended = false;
                let mut need_full_sync = false;
                {
                    let mut chain = node.chain.lock().unwrap();
                    if blocks[0].header.height == 1 && blocks[0].header.height <= chain.height() {
                        // Vollständige Kette empfangen -> ggf. Reorg
                        let mut full = vec![crate::block::genesis()];
                        full.extend(blocks.clone());
                        if chain.try_replace(full).is_ok() {
                            appended = true;
                        }
                    } else {
                        for b in blocks {
                            match chain.add_block(b) {
                                Ok(()) => appended = true,
                                Err(_) => {
                                    if !appended {
                                        need_full_sync = true;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                if appended {
                    node.save_chain();
                    let my_height = node.chain.lock().unwrap().height();
                    node.log(format!("Synchronisiert bis Block {my_height}."));
                    let _ = tx_out.send(Msg::GetBlocks {
                        from: my_height + 1,
                    });
                } else if need_full_sync {
                    // Ketten weichen ab -> komplette Kette anfordern
                    let _ = tx_out.send(Msg::GetBlocks { from: 1 });
                }
            }
            Msg::NewBlock(block) => {
                let height = block.header.height;
                let result = {
                    let mut chain = node.chain.lock().unwrap();
                    chain.add_block(block.clone())
                };
                match result {
                    Ok(()) => {
                        node.save_chain();
                        node.log(format!("Neuer Block {height} aus dem Netzwerk."));
                        node.broadcast(&Msg::NewBlock(block), Some(id));
                    }
                    Err(_) => {
                        let my_height = node.chain.lock().unwrap().height();
                        if height > my_height {
                            let _ = tx_out.send(Msg::GetBlocks {
                                from: my_height + 1,
                            });
                        }
                    }
                }
            }
            Msg::NewTx(tx) => {
                let added = {
                    let mut chain = node.chain.lock().unwrap();
                    chain.add_to_mempool(tx.clone()).is_ok()
                };
                if added {
                    node.broadcast(&Msg::NewTx(tx), Some(id));
                }
            }
            Msg::GetPeers => {
                let known: Vec<String> = node
                    .known_peers
                    .lock()
                    .unwrap()
                    .iter()
                    .take(50)
                    .cloned()
                    .collect();
                let _ = tx_out.send(Msg::Peers(known));
            }
            Msg::Peers(list) => {
                let mut known = node.known_peers.lock().unwrap();
                for addr in list.into_iter().take(50) {
                    if addr.parse::<std::net::SocketAddr>().is_ok() {
                        known.insert(addr);
                    }
                }
            }
        }
    }

    node.peers.lock().unwrap().remove(&id);
    node.log(format!("Peer getrennt: {display_addr}"));
    drop(tx_out);
    let _ = writer.join();
}
