//! Transaktionen, Adressen und Signaturen.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub type Hash32 = [u8; 32];
pub type Address = [u8; 20];

pub fn sha256d(data: &[u8]) -> Hash32 {
    let first = Sha256::digest(data);
    let second = Sha256::digest(first);
    second.into()
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct OutPoint {
    pub txid: Hash32,
    pub vout: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TxInput {
    pub prev: OutPoint,
    pub pubkey: [u8; 32],
    pub signature: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TxOutput {
    pub amount: u64,
    pub to: Address,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Transaction {
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub timestamp: u64,
}

impl Transaction {
    pub fn txid(&self) -> Hash32 {
        sha256d(&bincode::serialize(self).unwrap())
    }

    /// Hash, der von allen Inputs signiert wird (Signaturen ausgenullt)
    pub fn sighash(&self) -> Hash32 {
        let mut copy = self.clone();
        for input in &mut copy.inputs {
            input.signature.clear();
        }
        sha256d(&bincode::serialize(&copy).unwrap())
    }

    pub fn is_coinbase(&self) -> bool {
        self.inputs.len() == 1 && self.inputs[0].prev.txid == [0u8; 32]
    }

    /// Coinbase-Transaktion: erzeugt neue Coins für den Miner
    pub fn coinbase(height: u64, to: Address, amount: u64) -> Self {
        Transaction {
            inputs: vec![TxInput {
                prev: OutPoint {
                    txid: [0u8; 32],
                    vout: height as u32,
                },
                pubkey: [0u8; 32],
                signature: height.to_le_bytes().to_vec(),
            }],
            outputs: vec![TxOutput { amount, to }],
            timestamp: crate::params::now_ts(),
        }
    }

    /// Prüft alle Signaturen gegen den sighash
    pub fn verify_signatures(&self) -> bool {
        let hash = self.sighash();
        for input in &self.inputs {
            let Ok(vk) = VerifyingKey::from_bytes(&input.pubkey) else {
                return false;
            };
            let Ok(sig) = Signature::from_slice(&input.signature) else {
                return false;
            };
            if vk.verify(&hash, &sig).is_err() {
                return false;
            }
        }
        true
    }
}

/// Adresse = erste 20 Bytes von SHA-256(pubkey)
pub fn address_of_pubkey(pubkey: &[u8; 32]) -> Address {
    let h = Sha256::digest(pubkey);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[..20]);
    a
}

/// Adressformat: "T" + hex(20 Byte Adresse + 2 Byte Prüfsumme)
pub fn encode_address(a: &Address) -> String {
    let ck = Sha256::digest(a);
    let mut v = a.to_vec();
    v.extend_from_slice(&ck[..2]);
    format!("T{}", hex::encode(v))
}

pub fn decode_address(s: &str) -> Option<Address> {
    let s = s.trim();
    let body = s.strip_prefix('T').or_else(|| s.strip_prefix('t'))?;
    let bytes = hex::decode(body).ok()?;
    if bytes.len() != 22 {
        return None;
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&bytes[..20]);
    let ck = Sha256::digest(a);
    if ck[..2] != bytes[20..] {
        return None;
    }
    Some(a)
}
