//! Titanium Core (TCORE) — Wallet, Miner und vollwertiger Netzwerk-Node in einem Programm.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod block;
mod chain;
mod miner;
mod nat;
mod network;
mod node;
mod params;
mod storage;
mod theme;
mod tx;
mod wallet;

use node::Node;
use std::sync::Arc;

fn main() -> eframe::Result {
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
