//! Deterministic unsigned messages; only a consumed budget permit enables signing.
use crate::{
    governor::Permit,
    policy::{MIN_TIP_LAMPORTS, cu_limit},
};
use alight_types::{CanaryConfig, Route, SizeClass, Source, TipTier};
use sha2::{Digest, Sha256};
use solami::{Instruction, Keypair, Pubkey, Transaction, VersionedTransaction};
use solana_hash::Hash;
use solana_message::Message;
use solana_signer::Signer;
use std::str::FromStr;
use thiserror::Error;
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("invalid canary wallet, assignment, or blockhash")]
    Invalid,
    #[error("transaction exceeds Solana's 1232-byte packet limit")]
    Oversize,
    #[error("budget permit does not authorize this transaction")]
    Permit,
}
/// No Debug/Serialize: the only signing identity is the dedicated canary payer.
pub struct Wallet {
    payer: Keypair,
    owned: [Pubkey; 4],
}
impl Wallet {
    /// Exact memo-only message and Live/RPC permit; no arbitrary instruction can enter this path.
    pub(crate) fn sign_anchor(
        &self,
        permit: Permit,
        draft: &alight_types::AnchorDraft,
        message: Message,
        fee: u64,
    ) -> Result<VersionedTransaction, BuildError> {
        let reservation = permit.consume();
        let expected =
            crate::anchor::message(draft, self.public(), message.recent_blockhash.clone())
                .map_err(|_| BuildError::Invalid)?;
        if reservation.source != Source::Live
            || reservation.route != Route::Rpc
            || reservation.id != crate::anchor::reservation_id(draft)
            || reservation.lamports != fee
            || !(1..=100_000).contains(&fee)
            || message != expected
        {
            return Err(BuildError::Permit);
        }
        let bytes = bincode::serialize(&message).map_err(|_| BuildError::Invalid)?;
        let mut tx = Transaction::new_unsigned(message);
        tx.signatures[0] = self
            .payer
            .try_sign_message(&bytes)
            .map_err(|_| BuildError::Invalid)?;
        if !tx.signatures[0].verify(self.public().as_ref(), &bytes) {
            return Err(BuildError::Invalid);
        }
        Ok(VersionedTransaction::from(tx))
    }
    pub fn from_base58(secret: &str, expected_public: &str) -> Result<Self, BuildError> {
        let payer = Keypair::try_from_base58_string(secret).map_err(|_| BuildError::Invalid)?;
        if payer.pubkey().to_string() != expected_public {
            return Err(BuildError::Invalid);
        }
        Ok(Self::from_keypair(payer))
    }
    pub fn from_keypair(payer: Keypair) -> Self {
        let owned = std::array::from_fn(|i| {
            let mut hash = Sha256::new();
            hash.update(b"alight-owned-lock-v1");
            hash.update(payer.secret_bytes());
            hash.update([i as u8]);
            Keypair::new_from_array(hash.finalize().into()).pubkey()
        });
        Self { payer, owned }
    }
    pub fn public(&self) -> Pubkey {
        self.payer.pubkey()
    }
    /// Extra lock addresses are derived from private material controlled by this wallet.
    pub fn message(
        &self,
        config: &CanaryConfig,
        id: &str,
        blockhash: &str,
        tip: Option<Pubkey>,
    ) -> Result<Message, BuildError> {
        if id.is_empty()
            || id.len() > 64
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || config.cu_limit != cu_limit(config.size_class)
        {
            return Err(BuildError::Invalid);
        }
        let required_tip = match config.tip_tier {
            TipTier::None => 0,
            TipTier::X1 => MIN_TIP_LAMPORTS,
            TipTier::X2 => 2 * MIN_TIP_LAMPORTS,
            TipTier::X5 => 5 * MIN_TIP_LAMPORTS,
            TipTier::X10 => 10 * MIN_TIP_LAMPORTS,
        };
        if config.tip_lamports != required_tip
            || (config.route == Route::Rpc) != (config.tip_tier == TipTier::None)
            || (config.route == Route::Rpc) != tip.is_none()
        {
            return Err(BuildError::Invalid);
        }
        let compute = Pubkey::from_str("ComputeBudget111111111111111111111111111111")
            .map_err(|_| BuildError::Invalid)?;
        let mut limit = vec![2];
        limit.extend(config.cu_limit.to_le_bytes());
        let mut price = vec![3];
        price.extend(config.cu_price_micro_lamports.to_le_bytes());
        let mut instructions = vec![
            Instruction {
                program_id: compute,
                accounts: vec![],
                data: limit,
            },
            Instruction {
                program_id: compute,
                accounts: vec![],
                data: price,
            },
        ];
        let mut memo = format!("alight:{id}").into_bytes();
        if config.size_class == SizeClass::Large {
            memo.resize(384, b'.');
        }
        instructions.push(Instruction {
            program_id: Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
                .map_err(|_| BuildError::Invalid)?,
            accounts: vec![],
            data: memo,
        });
        // Tip precedes zero transfers: Solami's SDK scans the first system transfer.
        if let Some(tip) = tip {
            if tip == self.public() || self.owned.contains(&tip) {
                return Err(BuildError::Invalid);
            }
            instructions.push(solami::system_instruction::transfer(
                &self.public(),
                &tip,
                required_tip,
            ));
        }
        if config.size_class != SizeClass::Small {
            let mut extra = solami::system_instruction::transfer(&self.public(), &self.public(), 0);
            // System transfer consumes its first two accounts; extras add read-only load cost.
            for key in [
                "SysvarC1ock11111111111111111111111111111111",
                "SysvarRent111111111111111111111111111111111",
                "SysvarEpochSchedu1e111111111111111111111111",
                "SysvarS1otHashes111111111111111111111111111",
            ] {
                extra
                    .accounts
                    .push(solana_instruction::AccountMeta::new_readonly(
                        Pubkey::from_str(key).map_err(|_| BuildError::Invalid)?,
                        false,
                    ));
            }
            instructions.push(extra);
        }
        if config.size_class == SizeClass::Large {
            for owned in self.owned {
                instructions.push(solami::system_instruction::transfer(
                    &self.public(),
                    &owned,
                    0,
                ));
            }
        }
        let hash = Hash::from_str(blockhash).map_err(|_| BuildError::Invalid)?;
        let message = Message::new_with_blockhash(&instructions, Some(&self.public()), &hash);
        let unsigned = VersionedTransaction::from(Transaction::new_unsigned(message.clone()));
        if bincode::serialize(&unsigned)
            .map_err(|_| BuildError::Invalid)?
            .len()
            > 1232
        {
            return Err(BuildError::Oversize);
        }
        Ok(message)
    }
    /// Consumes the permit before accessing the signer. Source remains tied to the reservation.
    pub(crate) fn sign(
        &self,
        permit: Permit,
        id: &str,
        config: &CanaryConfig,
        message: Message,
        fee: u64,
    ) -> Result<(VersionedTransaction, Source), BuildError> {
        let reservation = permit.consume();
        let required = fee
            .checked_add(config.tip_lamports)
            .ok_or(BuildError::Invalid)?;
        if reservation.id != id
            || reservation.route != config.route
            || reservation.lamports < required
            || ![Source::Live, Source::Sim].contains(&reservation.source)
            || message.account_keys.first() != Some(&self.public())
        {
            return Err(BuildError::Permit);
        }
        let mut tx = Transaction::new_unsigned(message);
        if tx.message.header.num_required_signatures != 1 {
            return Err(BuildError::Invalid);
        }
        let bytes = bincode::serialize(&tx.message).map_err(|_| BuildError::Invalid)?;
        tx.signatures[0] = self
            .payer
            .try_sign_message(&bytes)
            .map_err(|_| BuildError::Invalid)?;
        if !tx.signatures[0].verify(self.public().as_ref(), &bytes) {
            return Err(BuildError::Invalid);
        }
        Ok((VersionedTransaction::from(tx), reservation.source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{governor::Governor, policy::Policy};
    use alight_store::Store;
    use alight_types::RunMode;
    #[tokio::test]
    async fn valid_budget_permit_produces_verified_signature_and_rejects_wrong_route() {
        let dir = tempfile::tempdir().expect("directory");
        let store = Store::open(&dir.path().join("sign.db"), 16 * 1024 * 1024)
            .await
            .expect("store");
        let governor = Governor::new(store, RunMode::Sim, Some("0.001"), Some("0.001"), 60_000)
            .expect("governor");
        let wallet = Wallet::from_keypair(Keypair::new_from_array([1; 32]));
        let policy = Policy::new(1, 0.30).expect("policy");
        let config = policy
            .cells()
            .iter()
            .find(|c| c.route == Route::Rpc && c.size_class == SizeClass::Small)
            .expect("RPC");
        let hash = bs58::encode([2u8; 32]).into_string();
        let message = wallet
            .message(config, "simulation-sign", &hash, None)
            .expect("message");
        let permit = governor
            .reserve("simulation-sign", Route::Rpc, 5000, 1_791_108_000_000)
            .await
            .expect("permit");
        let (signed, source) = wallet
            .sign(permit, "simulation-sign", config, message.clone(), 5000)
            .expect("sign");
        assert_eq!(source, Source::Sim);
        assert!(signed.signatures[0].verify(
            wallet.public().as_ref(),
            &bincode::serialize(&message).expect("message bytes")
        ));
        let permit = governor
            .reserve("wrong-route", Route::BeamQuic, 5000, 1_791_108_000_000)
            .await
            .expect("permit");
        assert!(matches!(
            wallet.sign(permit, "wrong-route", config, message, 5000),
            Err(BuildError::Permit)
        ));
    }
}
