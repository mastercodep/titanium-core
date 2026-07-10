# Titanium Core (TCORE)

Eine komplett eigenständige, dezentrale Kryptowährung — geschrieben in Rust.
Kein Fork, keine fremde Chain: eigenes Protokoll, eigener Genesis-Block, eigenes Netzwerk.

## Das Programm

Ein einziges Programm (`titanium-core.exe`) ist gleichzeitig:

- **Wallet** — Adresse erstellen, TCORE senden und empfangen
- **Full Node** — validiert jede Transaktion und jeden Block selbst
- **Miner** — erzeugt per Proof-of-Work (SHA-256d) neue Blöcke und damit neue TCORE
- **Netzwerkteilnehmer** — verbindet sich per P2P (TCP, Port 24333) mit anderen Nodes

## Konsens-Regeln

| Parameter | Wert |
|---|---|
| Maximale Menge | 21.000.000 TCORE |
| Start-Blockbelohnung | 50 TCORE |
| Halving | alle 210.000 Blöcke |
| Ziel-Blockzeit | 60 Sekunden |
| Difficulty-Anpassung | alle 30 Blöcke |
| Proof-of-Work | doppeltes SHA-256 |
| Signaturen | Ed25519 |
| Kleinste Einheit | 0,00000001 TCORE ("Shard") |

Der Genesis-Block (Block 0) enthält **keine Coins** und ist auf allen Nodes identisch.
Die allerersten TCORE entstehen, wenn der erste Node Block 1 mint — genau wie du es
beim ersten Start machst.

## Starten

```
cargo run --release
```

Beim ersten Start: **"Neues Wallet erstellen"** klicken, dann im Tab **Mining**
auf **"Mining starten"** — damit bringst du die ersten TCORE in Umlauf.

## Netzwerk & automatische Synchronisation

Jede frische Installation verbindet sich **vollautomatisch**: Im Programm sind
Seed-Nodes fest einkompiliert (`SEED_NODES` in `src/params.rs`, aktuell der PC des
Netzwerk-Gründers). Beim Start passiert automatisch:

1. Node verbindet sich mit den Seed-Nodes
2. lädt die komplette Blockchain herunter und prüft jeden Block selbst nach
3. lernt über Peer-Austausch weitere Teilnehmer kennen (gespeichert in `peers.txt`)
4. ab dann: Gossip — neue Blöcke/Transaktionen erreichen alle in Sekunden

Dadurch zeigt **jeder Node dieselben Zahlen** (Blockhöhe, Umlaufmenge, Difficulty) —
die längste gültige Kette gewinnt, wie bei Bitcoin. Zusätzliche Peers können manuell
im Tab **Netzwerk** eingetragen werden. Über `PEER_LIST_URLS` in `src/params.rs` kann
außerdem eine online gehostete Peer-Liste (eine `IP:Port` pro Zeile, z. B. als
GitHub-Raw-URL) hinterlegt werden — so lassen sich neue Seeds verteilen, ohne dass
jeder neu kompilieren muss.

**Wichtig für den Seed-Node-Betreiber:** Port **24333** (TCP) muss im Router auf
deinen PC weitergeleitet und in der Windows-Firewall erlaubt sein, und das Programm
muss laufen, damit Neulinge sich verbinden können. Ändert sich deine öffentliche IP,
muss die Adresse in `SEED_NODES` aktualisiert und neu gebaut werden (oder du nutzt
DynDNS / die `PEER_LIST_URLS`-Methode).

## Daten

Alles liegt unter `%APPDATA%\TitaniumCore`:

- `wallet.dat` — dein geheimer Schlüssel (**unbedingt sichern!**)
- `chain.dat` — die Blockchain
- `peers.txt` — bekannte Netzwerkteilnehmer

## Wichtige Hinweise

- Wer `wallet.dat` bzw. den geheimen Schlüssel besitzt, kontrolliert das Guthaben.
- Ein Marktkurs entsteht erst, wenn TCORE irgendwo gehandelt wird — der Statistik-Tab
  zeigt bis dahin Umlaufmenge, Hashrate und Difficulty.
- Dies ist ein junges Netzwerk: Solange nur wenige Miner aktiv sind, ist die Kette
  entsprechend leicht anzugreifen (51 %). Sicherheit wächst mit der Zahl der Teilnehmer.
