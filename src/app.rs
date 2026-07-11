//! Die Wallet-Oberfläche von Titanium Core.

use crate::node::{short_hash, Node};
use crate::params::*;
use crate::storage;
use crate::theme;
use crate::tx::{decode_address, encode_address};
use crate::wallet::Wallet;
use egui::{Align, Color32, Layout, RichText, TextEdit};
use egui_plot::{Line, Plot, PlotPoints};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Overview,
    Send,
    Receive,
    Mining,
    Network,
    Stats,
}

pub struct App {
    node: Arc<Node>,
    tab: Tab,
    send_to: String,
    send_amount: String,
    send_feedback: Option<(bool, String)>,
    import_secret: String,
    welcome_error: String,
    add_peer: String,
    last_hashes: u64,
    last_hash_time: Instant,
    hashrate: f64,
    copied_at: Option<Instant>,
    show_secret: bool,
    supply_cache: (u64, Vec<[f64; 2]>),
}

impl App {
    pub fn new(node: Arc<Node>) -> Self {
        App {
            node,
            tab: Tab::Overview,
            send_to: String::new(),
            send_amount: String::new(),
            send_feedback: None,
            import_secret: String::new(),
            welcome_error: String::new(),
            add_peer: String::new(),
            last_hashes: 0,
            last_hash_time: Instant::now(),
            hashrate: 0.0,
            copied_at: None,
            show_secret: false,
            supply_cache: (u64::MAX, Vec::new()),
        }
    }

    fn update_hashrate(&mut self) {
        let dt = self.last_hash_time.elapsed().as_secs_f64();
        if dt >= 1.0 {
            let total = self.node.hash_counter.load(Ordering::Relaxed);
            self.hashrate = (total.saturating_sub(self.last_hashes)) as f64 / dt;
            self.last_hashes = total;
            self.last_hash_time = Instant::now();
        }
        if !self.node.mining.load(Ordering::Relaxed) && self.last_hash_time.elapsed().as_secs_f64() < 1.0 {
            // beim Stoppen langsam auf 0
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(400));
        self.update_hashrate();

        let has_wallet = self.node.wallet.lock().unwrap().is_some();
        if !has_wallet {
            self.welcome_screen(ctx);
            return;
        }

        egui::SidePanel::left("nav")
            .exact_width(230.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG))
            .show(ctx, |ui| self.nav_panel(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::PANEL).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                    Tab::Overview => self.tab_overview(ui),
                    Tab::Send => self.tab_send(ui),
                    Tab::Receive => self.tab_receive(ui),
                    Tab::Mining => self.tab_mining(ui),
                    Tab::Network => self.tab_network(ui),
                    Tab::Stats => self.tab_stats(ui),
                });
            });
    }
}

impl App {
    // ---------- Willkommen / Wallet erstellen ----------
    fn welcome_screen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let header = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width(), 6.0),
                );
                theme::titanium_gradient(ui.painter(), header, 0.0);
                ui.add_space(rect.height() * 0.18);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("TITANIUM CORE")
                            .size(44.0)
                            .strong()
                            .color(theme::TITAN_LIGHT),
                    );
                    ui.label(
                        RichText::new("Die dezentrale Kryptowährung — geschmiedet aus Titan")
                            .size(16.0)
                            .color(theme::TITAN_DARK),
                    );
                    ui.add_space(40.0);
                    let create = egui::Button::new(
                        RichText::new("  Neues Wallet erstellen  ").size(18.0).strong(),
                    )
                    .min_size(egui::vec2(320.0, 52.0));
                    if ui.add(create).clicked() {
                        let w = Wallet::generate();
                        storage::save_wallet_secret(&self.node.data_dir, &w.secret_hex());
                        self.node.log(format!("Wallet erstellt: {}", w.address_string()));
                        *self.node.wallet.lock().unwrap() = Some(w);
                    }
                    ui.add_space(24.0);
                    ui.label(RichText::new("— oder vorhandenes Wallet importieren —").color(theme::TITAN_DARK));
                    ui.add_space(8.0);
                    ui.add(
                        TextEdit::singleline(&mut self.import_secret)
                            .hint_text("Geheimer Schlüssel (64 Hex-Zeichen)")
                            .desired_width(420.0)
                            .password(true),
                    );
                    if ui.button("Wallet importieren").clicked() {
                        match Wallet::from_secret_hex(&self.import_secret) {
                            Some(w) => {
                                storage::save_wallet_secret(&self.node.data_dir, &w.secret_hex());
                                self.node.log(format!("Wallet importiert: {}", w.address_string()));
                                *self.node.wallet.lock().unwrap() = Some(w);
                                self.welcome_error.clear();
                            }
                            None => {
                                self.welcome_error = "Ungültiger Schlüssel.".into();
                            }
                        }
                    }
                    if !self.welcome_error.is_empty() {
                        ui.colored_label(theme::RED, &self.welcome_error);
                    }
                });
            });
    }

    // ---------- Navigation ----------
    fn nav_panel(&mut self, ui: &mut egui::Ui) {
        let header_rect = egui::Rect::from_min_size(
            ui.max_rect().min,
            egui::vec2(ui.max_rect().width(), 86.0),
        );
        theme::titanium_gradient(ui.painter(), header_rect, 0.0);
        ui.allocate_ui_at_rect(header_rect.shrink(12.0), |ui| {
            ui.vertical(|ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new("TITANIUM CORE")
                        .size(21.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(RichText::new("TCORE  ·  v1.0").size(12.0).color(Color32::from_rgb(220, 226, 234)));
            });
        });
        ui.add_space(96.0);

        let items = [
            (Tab::Overview, "◆  Übersicht"),
            (Tab::Send, "↗  Senden"),
            (Tab::Receive, "↙  Empfangen"),
            (Tab::Mining, "⛏  Mining"),
            (Tab::Network, "🌐  Netzwerk"),
            (Tab::Stats, "📈  Kurs & Statistik"),
        ];
        for (tab, label) in items {
            let selected = self.tab == tab;
            let text = if selected {
                RichText::new(label).size(15.0).strong().color(Color32::WHITE)
            } else {
                RichText::new(label).size(15.0).color(theme::TITAN)
            };
            let btn = egui::Button::new(text)
                .fill(if selected { theme::CARD_HOVER } else { Color32::TRANSPARENT })
                .min_size(egui::vec2(ui.available_width(), 40.0));
            if ui.add(btn).clicked() {
                self.tab = tab;
                self.send_feedback = None;
            }
        }

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(14.0);
            let (height, mempool) = {
                let chain = self.node.chain.lock().unwrap();
                (chain.height(), chain.mempool.len())
            };
            let peers = self.node.peer_count();
            let mining = self.node.mining.load(Ordering::Relaxed);
            ui.label(RichText::new(format!("Block-Höhe: {height}")).size(12.0).color(theme::TITAN_DARK));
            ui.label(RichText::new(format!("Peers: {peers}  ·  Mempool: {mempool}")).size(12.0).color(theme::TITAN_DARK));
            if mining {
                ui.label(RichText::new(format!("⛏ Mining aktiv · {}", fmt_hashrate(self.hashrate))).size(12.0).color(theme::GREEN));
            } else {
                ui.label(RichText::new("⛏ Mining inaktiv").size(12.0).color(theme::TITAN_DARK));
            }
        });
    }

    // ---------- Übersicht ----------
    fn tab_overview(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Übersicht");
        let (balance, pending, history) = {
            let chain = self.node.chain.lock().unwrap();
            let wallet = self.node.wallet.lock().unwrap();
            let addr = wallet.as_ref().unwrap().address();
            let balance = chain.balance_of(&addr);
            let pending: u64 = chain
                .mempool
                .values()
                .flat_map(|t| t.outputs.iter())
                .filter(|o| o.to == addr)
                .map(|o| o.amount)
                .sum();
            (balance, pending, chain.history_of(&addr))
        };

        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Guthaben").size(13.0).color(theme::TITAN_DARK));
            ui.label(
                RichText::new(format!("{} TCORE", format_tcore(balance)))
                    .size(38.0)
                    .strong()
                    .color(theme::TITAN_LIGHT),
            );
            if pending > 0 {
                ui.label(
                    RichText::new(format!("+ {} TCORE eingehend (unbestätigt)", format_tcore(pending)))
                        .color(theme::ACCENT),
                );
            }
        });
        ui.add_space(16.0);

        section_title(ui, "Letzte Transaktionen");
        if history.is_empty() {
            ui.label(RichText::new("Noch keine Transaktionen. Starte das Mining, um die ersten TCORE zu erzeugen!").color(theme::TITAN_DARK));
        }
        for entry in history.iter().take(15) {
            theme::card_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (sign, color) = if entry.delta >= 0 {
                        ("+", theme::GREEN)
                    } else {
                        ("−", theme::RED)
                    };
                    ui.label(
                        RichText::new(format!("{sign} {} TCORE", format_tcore(entry.delta.unsigned_abs() as u64)))
                            .size(17.0)
                            .strong()
                            .color(color),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("Block {} · {}", entry.height, fmt_ts(entry.timestamp)))
                                .size(12.0)
                                .color(theme::TITAN_DARK),
                        );
                    });
                });
                ui.label(RichText::new(format!("Tx {}", short_hash(&entry.txid))).size(12.0).color(theme::TITAN_DARK));
            });
        }
    }

    // ---------- Senden ----------
    fn tab_send(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "TCORE senden");
        theme::card_frame().show(ui, |ui| {
            ui.label("Empfängeradresse");
            ui.add(
                TextEdit::singleline(&mut self.send_to)
                    .hint_text("T…")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            ui.add_space(8.0);
            ui.label("Betrag (TCORE)");
            ui.add(
                TextEdit::singleline(&mut self.send_amount)
                    .hint_text("0.0")
                    .desired_width(220.0),
            );
            ui.label(
                RichText::new(format!("Netzwerkgebühr: {} TCORE", format_tcore(MIN_FEE)))
                    .size(12.0)
                    .color(theme::TITAN_DARK),
            );
            ui.add_space(12.0);
            let send_btn = egui::Button::new(RichText::new("  Senden  ").size(16.0).strong())
                .min_size(egui::vec2(180.0, 42.0));
            if ui.add(send_btn).clicked() {
                self.send_feedback = Some(self.do_send());
                if matches!(self.send_feedback, Some((true, _))) {
                    self.send_to.clear();
                    self.send_amount.clear();
                }
            }
            if let Some((ok, msg)) = &self.send_feedback {
                ui.add_space(6.0);
                ui.colored_label(if *ok { theme::GREEN } else { theme::RED }, msg);
            }
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new("Hinweis: Die Transaktion wird bestätigt, sobald sie von einem Miner in einen Block aufgenommen wurde.")
                .size(12.0)
                .color(theme::TITAN_DARK),
        );
    }

    fn do_send(&mut self) -> (bool, String) {
        let Some(to) = decode_address(&self.send_to) else {
            return (false, "Ungültige Adresse.".into());
        };
        let Some(amount) = parse_tcore(&self.send_amount) else {
            return (false, "Ungültiger Betrag.".into());
        };
        let tx = {
            let chain = self.node.chain.lock().unwrap();
            let wallet = self.node.wallet.lock().unwrap();
            wallet.as_ref().unwrap().create_tx(&chain, to, amount, MIN_FEE)
        };
        match tx {
            Ok(tx) => match self.node.submit_tx(tx) {
                Ok(txid) => (
                    true,
                    format!("Gesendet! Transaktion {}", short_hash(&txid)),
                ),
                Err(e) => (false, e),
            },
            Err(e) => (false, e),
        }
    }

    // ---------- Empfangen ----------
    fn tab_receive(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "TCORE empfangen");
        let (addr_str, secret) = {
            let wallet = self.node.wallet.lock().unwrap();
            let w = wallet.as_ref().unwrap();
            (w.address_string(), w.secret_hex())
        };
        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Deine Adresse").size(13.0).color(theme::TITAN_DARK));
            ui.label(
                RichText::new(&addr_str)
                    .size(16.0)
                    .monospace()
                    .color(theme::TITAN_LIGHT),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("📋 Adresse kopieren").clicked() {
                    ui.output_mut(|o| o.copied_text = addr_str.clone());
                    self.copied_at = Some(Instant::now());
                }
                if let Some(t) = self.copied_at {
                    if t.elapsed().as_secs() < 2 {
                        ui.colored_label(theme::GREEN, "Kopiert!");
                    }
                }
            });
        });
        ui.add_space(16.0);
        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("⚠ Geheimer Schlüssel (Backup)").strong().color(theme::RED));
            ui.label(
                RichText::new("Wer diesen Schlüssel kennt, kontrolliert dein Guthaben. Sicher aufbewahren, niemals teilen!")
                    .size(12.0)
                    .color(theme::TITAN_DARK),
            );
            ui.add_space(6.0);
            if self.show_secret {
                ui.label(RichText::new(&secret).monospace().size(13.0));
                if ui.button("Verbergen").clicked() {
                    self.show_secret = false;
                }
            } else if ui.button("🔑 Schlüssel anzeigen").clicked() {
                self.show_secret = true;
            }
        });
    }

    // ---------- Mining ----------
    fn tab_mining(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Mining");
        let mining = self.node.mining.load(Ordering::Relaxed);
        let (height, difficulty, mempool) = {
            let chain = self.node.chain.lock().unwrap();
            (chain.height(), chain.difficulty(), chain.mempool.len())
        };
        let next_reward = block_reward(height + 1);
        let mined = self.node.blocks_mined.load(Ordering::Relaxed);
        let max_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);

        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let (label, color) = if mining {
                    ("■  Mining stoppen", theme::RED)
                } else {
                    ("▶  Mining starten", theme::GREEN)
                };
                let btn = egui::Button::new(RichText::new(label).size(17.0).strong().color(Color32::BLACK))
                    .fill(color)
                    .min_size(egui::vec2(220.0, 48.0));
                if ui.add(btn).clicked() {
                    self.node.mining.store(!mining, Ordering::Relaxed);
                    if !mining {
                        self.node.log("Mining gestartet.");
                    } else {
                        self.node.log("Mining gestoppt.");
                        self.hashrate = 0.0;
                    }
                }
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Eigene Hashrate").size(12.0).color(theme::TITAN_DARK));
                    ui.label(
                        RichText::new(if mining { fmt_hashrate(self.hashrate) } else { "—".into() })
                            .size(22.0)
                            .strong(),
                    );
                });
            });
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            // Rechen-Backend: GPU oder CPU
            let gpu_available = self.node.gpu_available.load(Ordering::Relaxed);
            if gpu_available {
                let gpu_name = self.node.gpu_name.lock().unwrap().clone();
                let mut use_gpu = self.node.use_gpu.load(Ordering::Relaxed);
                if ui
                    .checkbox(
                        &mut use_gpu,
                        RichText::new(format!("⚡ GPU-Mining nutzen  ({gpu_name})")).size(14.0),
                    )
                    .changed()
                {
                    self.node.use_gpu.store(use_gpu, Ordering::Relaxed);
                }
                let (txt, col) = if use_gpu {
                    ("Aktives Backend: GPU", theme::GREEN)
                } else {
                    ("Aktives Backend: CPU", theme::TITAN_DARK)
                };
                ui.label(RichText::new(txt).size(12.0).color(col));
            } else {
                ui.label(
                    RichText::new("Keine GPU/OpenCL erkannt — Mining läuft auf der CPU.")
                        .size(12.0)
                        .color(theme::TITAN_DARK),
                );
            }
            ui.add_space(8.0);
            let cpu_active = !gpu_available || !self.node.use_gpu.load(Ordering::Relaxed);
            ui.add_enabled_ui(cpu_active, |ui| {
                let mut threads = self.node.mining_threads.load(Ordering::Relaxed);
                ui.horizontal(|ui| {
                    ui.label("CPU-Threads:");
                    if ui.add(egui::Slider::new(&mut threads, 1..=max_threads)).changed() {
                        self.node.mining_threads.store(threads, Ordering::Relaxed);
                    }
                });
            });
        });
        ui.add_space(14.0);

        ui.columns(3, |cols| {
            stat_card(&mut cols[0], "Nächste Belohnung", &format!("{} TCORE", format_tcore(next_reward)));
            stat_card(&mut cols[1], "Difficulty", &format!("{difficulty:.2}"));
            stat_card(&mut cols[2], "Selbst gemint", &format!("{mined} Blöcke"));
        });
        ui.add_space(14.0);
        ui.label(
            RichText::new(format!(
                "Der Miner sichert das Netzwerk und nimmt {mempool} wartende Transaktionen in neue Blöcke auf. \
                 Jeder gefundene Block bringt dir die Blockbelohnung plus alle Gebühren."
            ))
            .size(12.0)
            .color(theme::TITAN_DARK),
        );
    }

    // ---------- Netzwerk ----------
    fn tab_network(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Dezentrales Netzwerk");
        theme::card_frame().show(ui, |ui| {
            ui.label(format!("Dein Node lauscht auf Port {} — andere können sich mit dir verbinden.", self.node.listen_port));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.add_peer)
                        .hint_text("IP:Port  (z. B. 192.168.1.20:24333)")
                        .desired_width(280.0),
                );
                if ui.button("Peer hinzufügen").clicked() && !self.add_peer.trim().is_empty() {
                    self.node
                        .known_peers
                        .lock()
                        .unwrap()
                        .insert(self.add_peer.trim().to_string());
                    self.node.save_peers();
                    self.node.log(format!("Peer gemerkt: {}", self.add_peer.trim()));
                    self.add_peer.clear();
                }
            });
        });
        ui.add_space(14.0);

        section_title(ui, "Verbundene Peers");
        let peers: Vec<(String, u64)> = self
            .node
            .peers
            .lock()
            .unwrap()
            .values()
            .map(|p| (p.addr.clone(), p.height))
            .collect();
        if peers.is_empty() {
            ui.label(RichText::new("Keine aktiven Verbindungen. Füge die IP:Port eines anderen Titanium-Core-Nodes hinzu.").color(theme::TITAN_DARK));
        }
        for (addr, height) in peers {
            theme::card_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🟢").size(12.0));
                    ui.label(RichText::new(addr).monospace());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("Höhe {height}")).color(theme::TITAN_DARK));
                    });
                });
            });
        }
        ui.add_space(14.0);

        section_title(ui, "Ereignisse");
        let log: Vec<String> = self.node.log.lock().unwrap().iter().rev().take(12).cloned().collect();
        for line in log {
            ui.label(RichText::new(format!("· {line}")).size(12.0).color(theme::TITAN_DARK));
        }
    }

    // ---------- Kurs & Statistik ----------
    fn tab_stats(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Kurs & Statistik");
        let (height, supply, difficulty, net_hashrate, mempool) = {
            let chain = self.node.chain.lock().unwrap();
            (
                chain.height(),
                chain.circulating_supply(),
                chain.difficulty(),
                chain.estimated_hashrate(),
                chain.mempool.len(),
            )
        };

        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Marktkurs").size(13.0).color(theme::TITAN_DARK));
            ui.label(RichText::new("—").size(34.0).strong());
            ui.label(
                RichText::new(
                    "TCORE wird noch an keiner Börse gehandelt — ein Marktpreis entsteht erst, \
                     wenn Menschen die Währung gegen andere Werte tauschen. Bis dahin zählt: \
                     Jeder TCORE ist durch echte Rechenarbeit (Proof-of-Work) entstanden.",
                )
                .size(12.0)
                .color(theme::TITAN_DARK),
            );
        });
        ui.add_space(14.0);

        ui.columns(4, |cols| {
            stat_card(&mut cols[0], "Umlaufmenge", &format!("{} TCORE", format_tcore(supply)));
            stat_card(&mut cols[1], "Max. Menge", "21.000.000 TCORE");
            stat_card(&mut cols[2], "Netzwerk-Hashrate", &fmt_hashrate(net_hashrate));
            stat_card(&mut cols[3], "Difficulty", &format!("{difficulty:.2}"));
        });
        ui.add_space(14.0);

        // Umlaufmenge über Blockhöhe (Cache, nur bei neuer Höhe neu berechnen)
        if self.supply_cache.0 != height {
            let chain = self.node.chain.lock().unwrap();
            let mut points = Vec::with_capacity(chain.blocks.len());
            let mut cum: u64 = 0;
            for b in &chain.blocks {
                if let Some(cb) = b.txs.first() {
                    if cb.is_coinbase() {
                        cum += cb.outputs.iter().map(|o| o.amount).sum::<u64>();
                    }
                }
                points.push([b.header.height as f64, (cum / COIN) as f64]);
            }
            self.supply_cache = (height, points);
        }

        section_title(ui, "Umlaufmenge über Blockhöhe");
        Plot::new("supply_plot")
            .height(260.0)
            .allow_scroll(false)
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(PlotPoints::from(self.supply_cache.1.clone()))
                        .color(theme::ACCENT)
                        .width(2.0),
                );
            });
        ui.add_space(10.0);
        ui.label(
            RichText::new(format!(
                "Block-Höhe: {height} · Wartende Transaktionen: {mempool} · Blockzeit-Ziel: {TARGET_BLOCK_TIME}s · Halving alle {HALVING_INTERVAL} Blöcke"
            ))
            .size(12.0)
            .color(theme::TITAN_DARK),
        );
    }
}

// ---------- Hilfsfunktionen ----------

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(22.0).strong().color(theme::TITAN_LIGHT));
    ui.add_space(8.0);
}

fn stat_card(ui: &mut egui::Ui, label: &str, value: &str) {
    theme::card_frame().show(ui, |ui| {
        ui.label(RichText::new(label).size(12.0).color(theme::TITAN_DARK));
        ui.label(RichText::new(value).size(17.0).strong().color(theme::TITAN_LIGHT));
    });
}

fn fmt_hashrate(h: f64) -> String {
    if h >= 1e9 {
        format!("{:.2} GH/s", h / 1e9)
    } else if h >= 1e6 {
        format!("{:.2} MH/s", h / 1e6)
    } else if h >= 1e3 {
        format!("{:.2} kH/s", h / 1e3)
    } else {
        format!("{h:.0} H/s")
    }
}

/// Unix-Zeitstempel -> "TT.MM.JJJJ HH:MM" (UTC), ohne externe Crates
fn fmt_ts(ts: u64) -> String {
    let days = (ts / 86_400) as i64;
    let secs = ts % 86_400;
    // Algorithmus von Howard Hinnant (civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:02}.{:02}.{} {:02}:{:02}",
        d,
        m,
        y,
        secs / 3600,
        (secs % 3600) / 60
    )
}
