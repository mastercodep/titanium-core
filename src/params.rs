//! Konsens-Parameter des Titanium-Core-Netzwerks.

/// Kleinste Einheit: 1 TCORE = 100_000_000 "shards"
pub const COIN: u64 = 100_000_000;
/// Maximale Gesamtmenge: 21 Millionen TCORE
pub const MAX_SUPPLY: u64 = 21_000_000 * COIN;
/// Start-Blockbelohnung: 50 TCORE
pub const INITIAL_REWARD: u64 = 50 * COIN;
/// Halving alle 210_000 Blöcke
pub const HALVING_INTERVAL: u64 = 210_000;
/// Ziel-Blockzeit in Sekunden
pub const TARGET_BLOCK_TIME: u64 = 60;
/// Difficulty-Anpassung alle N Blöcke
pub const RETARGET_INTERVAL: u64 = 30;
/// Start-Target (je kleiner, desto schwerer). Erste 16 Bytes des Hashs müssen <= Target sein.
pub const GENESIS_TARGET: u128 = u128::MAX >> 24;
/// Minimale Transaktionsgebühr (0.0001 TCORE)
pub const MIN_FEE: u64 = 10_000;
/// Standard-P2P-Port
pub const DEFAULT_PORT: u16 = 24333;
/// Protokollversion
pub const PROTOCOL_VERSION: u32 = 1;
/// Fester Genesis-Zeitstempel (identisch auf allen Nodes)
pub const GENESIS_TIMESTAMP: u64 = 1_767_225_600; // 2026-01-01 00:00:00 UTC
/// Genesis-Botschaft
pub const GENESIS_MESSAGE: &str = "Titanium Core - geschmiedet aus Titan, gesichert durch Arbeit.";

/// Fest eingebaute Seed-Nodes: erste Anlaufpunkte für frische Installationen.
/// Jeder neue Node verbindet sich hiermit automatisch und lernt dann weitere Peers kennen.
pub const SEED_NODES: &[&str] = &["tcore-seed.duckdns.org:24333"];

/// URLs, unter denen zusätzliche Peer-Listen abrufbar sind (eine Adresse pro Zeile).
/// So können neue Seed-Adressen verteilt werden, ohne das Programm neu zu kompilieren
/// (z. B. eine raw.githubusercontent.com-URL eintragen und neu bauen).
pub const PEER_LIST_URLS: &[&str] = &[];

/// Blockbelohnung auf gegebener Höhe (mit Halving)
pub fn block_reward(height: u64) -> u64 {
    let halvings = height / HALVING_INTERVAL;
    if halvings >= 64 {
        return 0;
    }
    INITIAL_REWARD >> halvings
}

/// Formatiert Shard-Betrag als TCORE-String
pub fn format_tcore(units: u64) -> String {
    let whole = units / COIN;
    let frac = units % COIN;
    if frac == 0 {
        format!("{whole}")
    } else {
        let s = format!("{frac:08}");
        let trimmed = s.trim_end_matches('0');
        format!("{whole}.{trimmed}")
    }
}

/// Parst einen TCORE-Betrag ("1.5") in Shards
pub fn parse_tcore(s: &str) -> Option<u64> {
    let s = s.trim().replace(',', ".");
    let mut parts = s.splitn(2, '.');
    let whole: u64 = parts.next()?.parse().ok()?;
    let frac_str = parts.next().unwrap_or("");
    if frac_str.len() > 8 || frac_str.chars().any(|c| !c.is_ascii_digit()) {
        return None;
    }
    let mut frac: u64 = if frac_str.is_empty() { 0 } else { frac_str.parse().ok()? };
    frac *= 10u64.pow(8 - frac_str.len() as u32);
    whole.checked_mul(COIN)?.checked_add(frac)
}

pub fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
