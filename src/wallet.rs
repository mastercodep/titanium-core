//! Wallet: Schlüsselverwaltung, Adresse, Transaktionserstellung.

use crate::chain::Chain;
use crate::params::*;
use crate::tx::{
    address_of_pubkey, encode_address, Address, Transaction, TxInput, TxOutput,
};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;

pub struct Wallet {
    signing: SigningKey,
}

impl Wallet {
    pub fn generate() -> Self {
        Wallet {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn from_secret_hex(hex_str: &str) -> Option<Self> {
        let bytes = hex::decode(hex_str.trim()).ok()?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(Wallet {
            signing: SigningKey::from_bytes(&arr),
        })
    }

    pub fn secret_hex(&self) -> String {
        hex::encode(self.signing.to_bytes())
    }

    pub fn pubkey(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn address(&self) -> Address {
        address_of_pubkey(&self.pubkey())
    }

    pub fn address_string(&self) -> String {
        encode_address(&self.address())
    }

    /// Erstellt eine signierte Transaktion an `to` über `amount` Shards.
    pub fn create_tx(
        &self,
        chain: &Chain,
        to: Address,
        amount: u64,
        fee: u64,
    ) -> Result<Transaction, String> {
        if amount == 0 {
            return Err("Betrag muss größer als 0 sein".into());
        }
        let needed = amount
            .checked_add(fee)
            .ok_or("Betrag zu groß")?;
        let mut utxos = chain.spendable_utxos(&self.address());
        utxos.sort_by_key(|(_, o)| std::cmp::Reverse(o.amount));
        let mut selected = Vec::new();
        let mut in_sum: u64 = 0;
        for (op, out) in utxos {
            selected.push((op, out.amount));
            in_sum += out.amount;
            if in_sum >= needed {
                break;
            }
        }
        if in_sum < needed {
            return Err(format!(
                "Nicht genug Guthaben: benötigt {} TCORE (inkl. Gebühr), verfügbar {} TCORE",
                format_tcore(needed),
                format_tcore(in_sum)
            ));
        }
        let mut outputs = vec![TxOutput { amount, to }];
        let change = in_sum - needed;
        if change > 0 {
            outputs.push(TxOutput {
                amount: change,
                to: self.address(),
            });
        }
        let mut tx = Transaction {
            inputs: selected
                .iter()
                .map(|(op, _)| TxInput {
                    prev: *op,
                    pubkey: self.pubkey(),
                    signature: Vec::new(),
                })
                .collect(),
            outputs,
            timestamp: now_ts(),
        };
        let sighash = tx.sighash();
        let sig = self.signing.sign(&sighash).to_bytes().to_vec();
        for input in &mut tx.inputs {
            input.signature = sig.clone();
        }
        Ok(tx)
    }
}
