//! Automatische Erreichbarkeit: UPnP-Portfreigabe und DynDNS-Updates.

use crate::node::Node;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

/// Öffnet den P2P-Port automatisch im Router (UPnP), sofern der Router das erlaubt.
/// Wird regelmäßig erneuert, damit die Freigabe auch nach Netzwerkwechseln
/// (Laptop unterwegs) im jeweils aktuellen Netz neu eingerichtet wird.
pub fn spawn_upnp(node: Arc<Node>) {
    std::thread::spawn(move || {
        let mut last_result: Option<bool> = None;
        loop {
            let ok = match try_map_port(node.listen_port) {
                Ok(ext_ip) => {
                    if last_result != Some(true) {
                        node.log(format!(
                            "UPnP: Port {} automatisch freigegeben (öffentliche IP {ext_ip})",
                            node.listen_port
                        ));
                    }
                    true
                }
                Err(_) => {
                    if last_result.is_none() {
                        node.log(
                            "UPnP nicht verfügbar - in diesem Netzwerk sind eingehende Verbindungen ggf. blockiert.",
                        );
                    }
                    false
                }
            };
            last_result = Some(ok);
            std::thread::sleep(Duration::from_secs(20 * 60));
        }
    });
}

fn try_map_port(port: u16) -> Result<Ipv4Addr, String> {
    let gateway = igd::search_gateway(igd::SearchOptions {
        timeout: Some(Duration::from_secs(5)),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let local_ip = local_ipv4().ok_or("Lokale IP nicht ermittelbar")?;
    gateway
        .add_port(
            igd::PortMappingProtocol::TCP,
            port,
            SocketAddrV4::new(local_ip, port),
            7200, // Freigabe für 2h, wird regelmäßig erneuert
            "Titanium Core",
        )
        .map_err(|e| e.to_string())?;
    gateway.get_external_ip().map_err(|e| e.to_string())
}

/// Ermittelt die lokale IPv4-Adresse (ohne Daten zu senden).
fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()? {
        std::net::SocketAddr::V4(a) => Some(*a.ip()),
        _ => None,
    }
}

/// DynDNS-Updater: Liegt in %APPDATA%\TitaniumCore eine Datei `dyndns.txt`
/// mit einer Update-URL (z. B. von DuckDNS), wird sie alle 5 Minuten aufgerufen.
/// So zeigt ein fester Domainname immer auf die aktuelle IP dieses Rechners,
/// egal in welchem Netzwerk er gerade hängt.
pub fn spawn_dyndns(node: Arc<Node>) {
    std::thread::spawn(move || {
        let path = node.data_dir.join("dyndns.txt");
        let mut announced = false;
        loop {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if !announced {
                    node.log("DynDNS-Update aktiv (dyndns.txt gefunden).");
                    announced = true;
                }
                for url in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let _ = ureq::get(url).timeout(Duration::from_secs(10)).call();
                }
            }
            std::thread::sleep(Duration::from_secs(300));
        }
    });
}
