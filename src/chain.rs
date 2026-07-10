//! Kettenzustand: Validierung, UTXO-Set, Mempool, Difficulty-Anpassung.

use crate::block::{genesis, hash_value, merkle_root, Block};
use crate::params::*;
use crate::tx::{address_of_pubkey, Address, Hash32, OutPoint, Transaction, TxOutput};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub height: u64,
    pub txid: Hash32,
    /// positiv = empfangen, negativ = gesendet
    pub delta: i128,
    pub timestamp: u64,
}

pub struct Chain {
    pub blocks: Vec<Block>,
    pub utxos: HashMap<OutPoint, TxOutput>,
    pub mempool: HashMap<Hash32, Transaction>,
    /// txid -> (Blockhöhe, Index im Block)
    pub tx_index: HashMap<Hash32, (u64, usize)>,
}

impl Chain {
    pub fn new() -> Self {
        let mut c = Chain {
            blocks: vec![genesis()],
            utxos: HashMap::new(),
            mempool: HashMap::new(),
            tx_index: HashMap::new(),
        };
        c.rebuild_indices();
        c
    }

    /// Baut eine Kette aus gespeicherten Blöcken und validiert sie komplett.
    pub fn from_blocks(blocks: Vec<Block>) -> Result<Self, String> {
        let mut c = Chain::new();
        for b in blocks.into_iter().skip(1) {
            c.add_block(b)?;
        }
        Ok(c)
    }

    fn rebuild_indices(&mut self) {
        self.utxos.clear();
        self.tx_index.clear();
        let blocks = self.blocks.clone();
        for b in &blocks {
            self.apply_block_to_state(b);
        }
    }

    fn apply_block_to_state(&mut self, b: &Block) {
        for (i, tx) in b.txs.iter().enumerate() {
            let txid = tx.txid();
            if !tx.is_coinbase() {
                for input in &tx.inputs {
                    self.utxos.remove(&input.prev);
                }
            }
            for (vout, out) in tx.outputs.iter().enumerate() {
                self.utxos.insert(
                    OutPoint {
                        txid,
                        vout: vout as u32,
                    },
                    out.clone(),
                );
            }
            self.tx_index.insert(txid, (b.header.height, i));
            self.mempool.remove(&txid);
        }
    }

    pub fn height(&self) -> u64 {
        self.blocks.last().unwrap().header.height
    }

    pub fn tip_hash(&self) -> Hash32 {
        self.blocks.last().unwrap().header.hash()
    }

    /// Erwartetes Target für die nächste Blockhöhe (Difficulty-Anpassung)
    pub fn expected_target(&self, next_height: u64) -> u128 {
        if next_height <= 1 {
            return GENESIS_TARGET;
        }
        let prev_target = self.blocks[(next_height - 1) as usize].header.target;
        if next_height % RETARGET_INTERVAL != 0 {
            return prev_target;
        }
        let window_start = next_height - RETARGET_INTERVAL;
        let t_start = self.blocks[window_start as usize].header.timestamp;
        let t_end = self.blocks[(next_height - 1) as usize].header.timestamp;
        let expected = RETARGET_INTERVAL * TARGET_BLOCK_TIME;
        let actual = (t_end.saturating_sub(t_start)).clamp(expected / 4, expected * 4);
        let new_target = prev_target / expected as u128 * actual as u128;
        new_target.clamp(1, GENESIS_TARGET)
    }

    /// Validiert eine Transaktion gegen das UTXO-Set. Gibt die Gebühr zurück.
    pub fn validate_tx(
        &self,
        tx: &Transaction,
        spent: &HashSet<OutPoint>,
    ) -> Result<u64, String> {
        if tx.is_coinbase() {
            return Err("Coinbase außerhalb eines Blocks".into());
        }
        if tx.inputs.is_empty() || tx.outputs.is_empty() {
            return Err("Leere Inputs/Outputs".into());
        }
        if !tx.verify_signatures() {
            return Err("Ungültige Signatur".into());
        }
        let mut in_sum: u64 = 0;
        let mut seen: HashSet<OutPoint> = HashSet::new();
        for input in &tx.inputs {
            if spent.contains(&input.prev) || !seen.insert(input.prev) {
                return Err("Doppelte Ausgabe".into());
            }
            let utxo = self
                .utxos
                .get(&input.prev)
                .ok_or("Input verweist auf unbekannten Output")?;
            if address_of_pubkey(&input.pubkey) != utxo.to {
                return Err("Pubkey passt nicht zur Adresse".into());
            }
            in_sum = in_sum.checked_add(utxo.amount).ok_or("Überlauf")?;
        }
        let mut out_sum: u64 = 0;
        for out in &tx.outputs {
            if out.amount == 0 {
                return Err("Output mit Betrag 0".into());
            }
            out_sum = out_sum.checked_add(out.amount).ok_or("Überlauf")?;
        }
        if out_sum > in_sum {
            return Err("Outputs übersteigen Inputs".into());
        }
        let fee = in_sum - out_sum;
        if fee < MIN_FEE {
            return Err("Gebühr zu niedrig".into());
        }
        Ok(fee)
    }

    /// Vollständige Blockvalidierung und Anwendung auf den Zustand.
    pub fn add_block(&mut self, block: Block) -> Result<(), String> {
        let h = &block.header;
        if h.height != self.height() + 1 {
            return Err(format!("Falsche Höhe {} (erwartet {})", h.height, self.height() + 1));
        }
        if h.prev_hash != self.tip_hash() {
            return Err("prev_hash passt nicht zur Kettenspitze".into());
        }
        if h.target != self.expected_target(h.height) {
            return Err("Falsches Target".into());
        }
        if !h.meets_target() {
            return Err("Proof-of-Work ungültig".into());
        }
        if h.timestamp > now_ts() + 7200 {
            return Err("Zeitstempel zu weit in der Zukunft".into());
        }
        if block.txs.is_empty() || !block.txs[0].is_coinbase() {
            return Err("Erster Tx muss Coinbase sein".into());
        }
        let txids: Vec<Hash32> = block.txs.iter().map(|t| t.txid()).collect();
        if merkle_root(&txids) != h.merkle_root {
            return Err("Merkle-Root ungültig".into());
        }
        let mut spent: HashSet<OutPoint> = HashSet::new();
        let mut fees: u64 = 0;
        for tx in block.txs.iter().skip(1) {
            let fee = self.validate_tx(tx, &spent)?;
            fees += fee;
            for input in &tx.inputs {
                spent.insert(input.prev);
            }
        }
        let coinbase_out: u64 = block.txs[0].outputs.iter().map(|o| o.amount).sum();
        if coinbase_out > block_reward(h.height) + fees {
            return Err("Coinbase zahlt zu viel aus".into());
        }
        self.apply_block_to_state(&block);
        self.blocks.push(block);
        Ok(())
    }

    /// Ersetzt die eigene Kette durch eine längere, gültige Kette (Reorg beim Sync).
    pub fn try_replace(&mut self, blocks: Vec<Block>) -> Result<(), String> {
        let candidate = Chain::from_blocks(blocks)?;
        if candidate.height() <= self.height() {
            return Err("Kandidat nicht länger".into());
        }
        let mempool = std::mem::take(&mut self.mempool);
        *self = candidate;
        for (_, tx) in mempool {
            let _ = self.add_to_mempool(tx);
        }
        Ok(())
    }

    pub fn add_to_mempool(&mut self, tx: Transaction) -> Result<Hash32, String> {
        let txid = tx.txid();
        if self.mempool.contains_key(&txid) {
            return Err("Bereits im Mempool".into());
        }
        let spent: HashSet<OutPoint> = self
            .mempool
            .values()
            .flat_map(|t| t.inputs.iter().map(|i| i.prev))
            .collect();
        self.validate_tx(&tx, &spent)?;
        self.mempool.insert(txid, tx);
        Ok(txid)
    }

    pub fn balance_of(&self, addr: &Address) -> u64 {
        self.utxos
            .values()
            .filter(|o| &o.to == addr)
            .map(|o| o.amount)
            .sum()
    }

    /// Guthaben abzüglich bereits im Mempool ausgegebener eigener Outputs
    pub fn spendable_utxos(&self, addr: &Address) -> Vec<(OutPoint, TxOutput)> {
        let pending_spent: HashSet<OutPoint> = self
            .mempool
            .values()
            .flat_map(|t| t.inputs.iter().map(|i| i.prev))
            .collect();
        self.utxos
            .iter()
            .filter(|(op, o)| &o.to == addr && !pending_spent.contains(op))
            .map(|(op, o)| (*op, o.clone()))
            .collect()
    }

    pub fn circulating_supply(&self) -> u64 {
        self.utxos.values().map(|o| o.amount).sum()
    }

    /// Transaktionshistorie einer Adresse
    pub fn history_of(&self, addr: &Address) -> Vec<HistoryEntry> {
        let mut result = Vec::new();
        for b in &self.blocks {
            for tx in &b.txs {
                let mut delta: i128 = 0;
                for out in &tx.outputs {
                    if &out.to == addr {
                        delta += out.amount as i128;
                    }
                }
                if !tx.is_coinbase() {
                    for input in &tx.inputs {
                        if address_of_pubkey(&input.pubkey) == *addr {
                            if let Some(&(h, i)) = self.tx_index.get(&input.prev.txid) {
                                let prev_out =
                                    &self.blocks[h as usize].txs[i].outputs[input.prev.vout as usize];
                                delta -= prev_out.amount as i128;
                            }
                        }
                    }
                }
                if delta != 0 {
                    result.push(HistoryEntry {
                        height: b.header.height,
                        txid: tx.txid(),
                        delta,
                        timestamp: tx.timestamp,
                    });
                }
            }
        }
        result.reverse();
        result
    }

    /// Geschätzte Netzwerk-Hashrate in H/s an der Kettenspitze
    pub fn estimated_hashrate(&self) -> f64 {
        let tip = &self.blocks.last().unwrap().header;
        if tip.height == 0 {
            return 0.0;
        }
        let hashes_per_block = (u128::MAX / tip.target.max(1)) as f64;
        hashes_per_block / TARGET_BLOCK_TIME as f64
    }

    /// Difficulty relativ zum Genesis-Target
    pub fn difficulty(&self) -> f64 {
        let tip = &self.blocks.last().unwrap().header;
        GENESIS_TARGET as f64 / tip.target.max(1) as f64
    }

    pub fn hash_of_tip_value(&self) -> u128 {
        hash_value(&self.tip_hash())
    }
}
