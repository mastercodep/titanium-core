//! Titanium Core (TCORE) — Wallet, Miner und vollwertiger Netzwerk-Node in einem Programm.
//!
//! Mit grafischer Oberfläche starten (Standard):  titanium-core
//! Als reiner Server-Node ohne GUI:               titanium-core --headless
//! Server-Node, der zusätzlich mint:              titanium-core --headless --mine
//!
//! Für Server ohne Grafikbibliotheken komplett ohne GUI bauen:
//!   cargo build --release --no-default-features

#![cfg_attr(all(not(debug_assertions), feature = "gui"), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
mod app;
mod block;
mod chain;
mod gpu;
mod miner;
mod nat;
mod network;
mod node;
mod params;
mod storage;
#[cfg(feature = "gui")]
mod theme;
mod tx;
mod wallet;

use node::Node;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Diagnose: GPU-Selbsttest, Korrektheitsprüfung und Benchmark
    if args.iter().any(|a| a == "--gputest") {
        gpu_selftest();
        return;
    }

    let headless = args.iter().any(|a| a == "--headless") || cfg!(not(feature = "gui"));
    let auto_mine = args.iter().any(|a| a == "--mine");

    let node = Arc::new(Node::new(params::DEFAULT_PORT));

    network::spawn_listener(node.clone());
    network::spawn_seed_fetch(node.clone());
    network::spawn_connector(node.clone());
    nat::spawn_upnp(node.clone());
    nat::spawn_dyndns(node.clone());
    miner::spawn_miner(node.clone());

    // Kette regelmäßig sichern
    {
        let node = node.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
            node.save_chain();
        });
    }

    if headless {
        run_headless(node, auto_mine);
    } else {
        #[cfg(feature = "gui")]
        if let Err(e) = run_gui(node) {
            eprintln!("GUI-Fehler: {e}");
        }
    }
}

/// Prüft die GPU: Selbsttest (im init), echte Nonce-Suche mit CPU-Gegenprüfung, Benchmark.
fn gpu_selftest() {
    use block::BlockHeader;
    println!("Suche GPU / OpenCL ...");
    let Some(g) = gpu::GpuMiner::init() else {
        println!("Keine nutzbare GPU gefunden (oder Selbsttest fehlgeschlagen). Es würde die CPU genutzt.");
        return;
    };
    println!("GPU erkannt: {}", g.device_name);
    println!("Selbsttest (GPU-Hash == CPU-Hash): BESTANDEN");

    // Korrektheitsprüfung: leichten Nonce auf der GPU finden und auf der CPU gegenprüfen
    let header = BlockHeader {
        version: 1,
        height: 1,
        prev_hash: [0u8; 32],
        merkle_root: [0x5au8; 32],
        timestamp: 1_700_000_000,
        target: u128::MAX >> 20, // relativ leicht
        nonce: 0,
    };
    let bytes = bincode::serialize(&header).unwrap();
    let mut hb = [0u8; 108];
    hb.copy_from_slice(&bytes);
    match g.search(&hb, header.target, 0) {
        Some(nonce) => {
            let mut h = header.clone();
            h.nonce = nonce;
            if h.meets_target() {
                println!("Nonce-Suche: GPU fand Nonce {nonce}, CPU bestätigt gültig ✓");
            } else {
                println!("FEHLER: GPU meldete Nonce {nonce}, CPU lehnt ab ✗");
                return;
            }
        }
        None => println!("(kein Treffer im ersten Batch — bei diesem Target selten, aber möglich)"),
    }

    // Benchmark: unerfüllbares Target, Durchsatz messen
    let bench_header = BlockHeader {
        target: 0,
        ..header
    };
    let bb = bincode::serialize(&bench_header).unwrap();
    let mut bhb = [0u8; 108];
    bhb.copy_from_slice(&bb);
    let start = std::time::Instant::now();
    let mut hashes: u64 = 0;
    let mut base: u64 = 0;
    while start.elapsed().as_secs_f64() < 3.0 {
        g.search(&bhb, 0, base);
        hashes += g.batch;
        base = base.wrapping_add(g.batch);
    }
    let rate = hashes as f64 / start.elapsed().as_secs_f64();
    println!("Benchmark: {:.2} MH/s auf der GPU", rate / 1e6);
}

/// Server-Betrieb ohne Oberfläche: läuft dauerhaft, gibt den Status auf der Konsole aus.
fn run_headless(node: Arc<Node>, auto_mine: bool) -> ! {
    println!(
        "Titanium Core Node (headless) gestartet · Port {} · Datenverzeichnis {}",
        node.listen_port,
        node.data_dir.display()
    );

    if auto_mine {
        if node.wallet.lock().unwrap().is_none() {
            let w = wallet::Wallet::generate();
            storage::save_wallet_secret(&node.data_dir, &w.secret_hex());
            println!("Neues Mining-Wallet erstellt: {}", w.address_string());
            *node.wallet.lock().unwrap() = Some(w);
        }
        node.mining.store(true, Ordering::Relaxed);
        println!("Mining aktiviert.");
    }

    let mut last_height = u64::MAX;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let (h, supply) = {
            let chain = node.chain.lock().unwrap();
            (chain.height(), chain.circulating_supply())
        };
        if h != last_height {
            println!(
                "[Node] Höhe {h} · Peers {} · Umlauf {} TCORE",
                node.peer_count(),
                params::format_tcore(supply)
            );
            last_height = h;
        }
    }
}

#[cfg(feature = "gui")]
fn run_gui(node: Arc<Node>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([940.0, 620.0])
            .with_title("Titanium Core"),
        ..Default::default()
    };

    eframe::run_native(
        "Titanium Core",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(node)))
        }),
    )
}
