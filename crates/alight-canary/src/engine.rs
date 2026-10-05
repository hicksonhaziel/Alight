//! A single live canary pipeline: preflight -> reserve -> sign -> persist -> broadcast.
use crate::{
    builder::{BuildError, Wallet},
    governor::{BudgetError, Governor},
    policy::{AdaptivePolicy, PolicyError},
    resolver,
    routes::{Routes, SendResult},
};
use alight_ingest::{Config, HttpProbe, ProbeError, leaders::LeaderSchedule};
use alight_store::{Store, StoreError};
use alight_types::*;
use base64::Engine as _;
use chrono::Utc;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, str::FromStr, time::Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid live canary configuration")]
    Configuration,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Probe(#[from] ProbeError),
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error(transparent)]
    Budget(#[from] BudgetError),
    #[error(transparent)]
    Policy(#[from] PolicyError),
}

pub struct Engine {
    store: Store,
    governor: Governor,
    wallet: Wallet,
    policy: AdaptivePolicy,
    policy_key: String,
    routes: Routes,
    http: HttpProbe,
    leaders: Option<LeaderSchedule>,
    clock_id: String,
    started: Instant,
    reserve_balance: u64,
    max_pending: usize,
}
impl Engine {
    /// Live mode alone loads a signing identity; observe/replay never construct this engine.
    pub async fn new(config: &Config, store: Store) -> Result<Self, EngineError> {
        let wallet = Wallet::from_base58(
            config
                .get("ALIGHT_CANARY_KEYPAIR")
                .ok_or(EngineError::Configuration)?,
            config
                .get("ALIGHT_CANARY_PUBKEY")
                .ok_or(EngineError::Configuration)?,
        )?;
        let seed = config
            .get("ALIGHT_SEED")
            .unwrap_or("1")
            .parse()
            .map_err(|_| EngineError::Configuration)?;
        let fraction = config
            .get("ALIGHT_UNIFORM_FRACTION")
            .unwrap_or("0.30")
            .parse()
            .map_err(|_| EngineError::Configuration)?;
        let mut policy = AdaptivePolicy::new(seed, fraction)?;
        let policy_key = format!(
            "{}:{seed}:{fraction:.17}",
            crate::policy::ADAPTIVE_POLICY_ID
        );
        policy.resume(store.next_policy_draw(&policy_key).await?)?;
        let governor = Governor::new(
            store.clone(),
            RunMode::Live,
            config.get("ALIGHT_DAILY_BUDGET_SOL"),
            config.get("ALIGHT_BURST_BUDGET_SOL"),
            60_000,
        )?;
        let reserve_balance = config
            .get("ALIGHT_CANARY_RESERVE_LAMPORTS")
            .unwrap_or("1000000")
            .parse()
            .map_err(|_| EngineError::Configuration)?;
        let max_pending = config
            .get("ALIGHT_MAX_PENDING_CANARIES")
            .unwrap_or("8")
            .parse::<usize>()
            .map_err(|_| EngineError::Configuration)?;
        if reserve_balance < 1_000_000 || !(1..=32).contains(&max_pending) {
            return Err(EngineError::Configuration);
        }
        let (clock_id, started) = alight_types::process_clock_origin();
        Ok(Self {
            store,
            governor,
            wallet,
            policy,
            policy_key,
            routes: Routes::new().map_err(|_| EngineError::Configuration)?,
            http: HttpProbe::new()?,
            leaders: None,
            clock_id,
            started,
            reserve_balance,
            max_pending,
        })
    }
    /// Returns a sanitized status. Every network send has a durable signature and reservation.
    pub async fn step(&mut self, config: &Config) -> Result<Value, EngineError> {
        let health = self
            .store
            .observer_health(ObserverKind::Grpc, Source::Live)
            .await?;
        let age = health["last_receive_utc"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_milliseconds());
        if health["open_gaps"].as_i64() != Some(0) || !age.is_some_and(|ms| (0..3000).contains(&ms))
        {
            return Ok(json!({"status":"WAITING_OBSERVER"}));
        }
        let Some(sent_slot) = self.store.cursor(ObserverKind::Grpc, Source::Live).await? else {
            return Ok(json!({"status":"WAITING_OBSERVER"}));
        };
        let pending = self
            .store
            .pending_canaries()
            .await?
            .into_iter()
            .filter(|c| c.source == Source::Live)
            .count();
        if pending >= self.max_pending {
            return Ok(json!({"status":"PENDING_LIMIT","pending":pending}));
        }
        let balance=self.http.rpc(config,"getBalance",json!([self.wallet.public().to_string(),{"commitment":"confirmed","minContextSlot":sent_slot}])).await?;
        let balance = balance["value"].as_u64().ok_or(ProbeError::Format)?;
        let balance = balance.saturating_sub(self.store.pending_reserved_lamports().await?);
        if balance <= self.reserve_balance {
            return Ok(
                json!({"status":"WAITING_FUNDS","balance_lamports":balance.to_string(),"reserve_lamports":self.reserve_balance.to_string()}),
            );
        }
        if self
            .leaders
            .as_ref()
            .is_none_or(|s| !s.covers(sent_slot.saturating_add(1)))
        {
            let leaders = LeaderSchedule::fetch(config).await?;
            self.store.save_evidence(&leaders.evidence()?).await?;
            self.leaders = Some(leaders);
        }
        let Some(leaders) = &self.leaders else {
            return Err(EngineError::Configuration);
        };
        if !leaders.covers(sent_slot) {
            return Ok(json!({"status":"WAITING_EPOCH"}));
        }
        let tip_addresses = self
            .http
            .get("https://api.solami.dev/onchain/tip-addresses", None)
            .await?;
        let tips = tip_addresses
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 64)
            .ok_or(ProbeError::Format)?;
        let id = uuid::Uuid::new_v4().to_string();
        let tip = solami::Pubkey::from_str(
            tips[(self.store.next_policy_draw(&self.policy_key).await? as usize) % tips.len()]
                .as_str()
                .ok_or(ProbeError::Format)?,
        )
        .map_err(|_| ProbeError::Format)?;
        let block = self
            .http
            .rpc(
                config,
                "getLatestBlockhash",
                json!([{"commitment":"confirmed","minContextSlot":sent_slot}]),
            )
            .await?;
        let blockhash = block["value"]["blockhash"]
            .as_str()
            .ok_or(ProbeError::Format)?;
        let last_valid_block_height = block["value"]["lastValidBlockHeight"]
            .as_u64()
            .ok_or(ProbeError::Format)?;
        // Determine the exact writable account set before measuring local fee buckets.
        let mut next = self.policy.clone();
        let samples = self.store.training_canaries(Source::Live).await?;
        let context = CurveContext {
            source: Source::Live,
            regime_id: "phase1-unclassified".into(),
            region: "local".into(),
            as_of_utc: Utc::now().to_rfc3339(),
        };
        let posterior =
            alight_model::pooled::policy_posteriors(&samples, next.cells(), &context, 2)
                .map_err(|_| EngineError::Configuration)?;
        let mut assignment = next.assign(0, 0, &posterior)?;
        let chosen_tip = if assignment.config.route == Route::Rpc {
            None
        } else {
            Some(tip)
        };
        let initial = self
            .wallet
            .message(&assignment.config, &id, blockhash, chosen_tip)?;
        let writable_count =
            initial.account_keys.len() - initial.header.num_readonly_unsigned_accounts as usize;
        let writable: Vec<String> = initial.account_keys[..writable_count]
            .iter()
            .map(ToString::to_string)
            .collect();
        let fees = self
            .http
            .rpc(config, "getRecentPrioritizationFees", json!([writable]))
            .await?;
        let mut values: Vec<u64> = fees
            .as_array()
            .ok_or(ProbeError::Format)?
            .iter()
            .map(|s| s["prioritizationFee"].as_u64().ok_or(ProbeError::Format))
            .collect::<Result<_, _>>()?;
        if values.is_empty() {
            return Err(ProbeError::Format.into());
        }
        values.sort_unstable();
        assignment.config.cu_price_micro_lamports = match assignment.config.fee_bucket {
            FeeBucket::Zero => 0,
            FeeBucket::LocalMedian => values[(values.len() - 1) / 2],
            FeeBucket::LocalP90 => values[(9 * (values.len() - 1)).div_ceil(10)],
        };
        let message = self
            .wallet
            .message(&assignment.config, &id, blockhash, chosen_tip)?;
        let fee_check=self.http.rpc(config,"getFeeForMessage",json!([base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&message).map_err(|_|BuildError::Invalid)?),{"commitment":"confirmed","minContextSlot":sent_slot}])).await?;
        let fee = fee_check["value"].as_u64().ok_or(ProbeError::Format)?;
        let priority = u64::try_from(
            (assignment.config.cu_price_micro_lamports as u128
                * assignment.config.cu_limit as u128)
                .div_ceil(1_000_000),
        )
        .map_err(|_| BuildError::Invalid)?;
        if fee < priority.checked_add(5000).ok_or(BuildError::Invalid)? {
            return Err(ProbeError::Format.into());
        }
        let worst_cost = fee
            .checked_add(assignment.config.tip_lamports)
            .ok_or(BuildError::Invalid)?;
        if worst_cost > balance.saturating_sub(self.reserve_balance) {
            return Ok(
                json!({"status":"WAITING_FUNDS","balance_lamports":balance.to_string(),"required_lamports":worst_cost.to_string()}),
            );
        }
        // Refresh the processed gRPC cursor after network preflight, at the send instant.
        let sent_slot = self
            .store
            .cursor(ObserverKind::Grpc, Source::Live)
            .await?
            .ok_or(EngineError::Configuration)?;
        if !leaders.covers(sent_slot) {
            return Ok(json!({"status":"WAITING_EPOCH"}));
        }
        let now = Utc::now();
        let utc_ms =
            u64::try_from(now.timestamp_millis()).map_err(|_| EngineError::Configuration)?;
        let permit = match self
            .governor
            .reserve(&id, assignment.config.route, worst_cost, utc_ms)
            .await
        {
            Ok(permit) => permit,
            Err(BudgetError::Denied) => return Ok(json!({"status":"BUDGET_CAPPED"})),
            Err(e) => return Err(e.into()),
        };
        // After this point failures remain charged. No restart re-broadcasts a prepared canary.
        let (tx, source) = self
            .wallet
            .sign(permit, &id, &assignment.config, message, fee)?;
        if source != Source::Live {
            return Err(EngineError::Configuration);
        }
        let signature = tx
            .signatures
            .first()
            .ok_or(BuildError::Invalid)?
            .to_string();
        let wire = bincode::serialize(&tx).map_err(|_| BuildError::Invalid)?;
        let canary = Canary {
            id: id.clone(),
            source,
            config: assignment.config.clone(),
            policy_id: assignment.policy_id.into(),
            assignment_prob: assignment.assignment_prob,
            uniform_arm: assignment.uniform_arm,
            regime_id: "phase1-unclassified".into(),
            sent_slot,
            clock_id: self.clock_id.clone(),
            send_mono_ns: u64::try_from(self.started.elapsed().as_nanos())
                .map_err(|_| EngineError::Configuration)?,
            send_wall_utc: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            signature: Some(signature),
            blockhash: blockhash.into(),
            last_valid_block_height,
            leader_class_next: leaders.next_leaders(sent_slot, 3),
            outcome: None,
            landed_slot: None,
            landed_block_id: None,
            landed_index: None,
            landed_index_scope: None,
            observer_first_seen: BTreeMap::new(),
            resolved_at_utc: None,
        };
        self.store
            .prepare_send(
                &canary,
                &self.policy_key,
                assignment.draw,
                &serde_json::to_value(&assignment).map_err(|_| EngineError::Configuration)?,
                &format!("sha256:{:x}", Sha256::digest(wire)),
            )
            .await?;
        self.store.save_evidence(&json!({"canary_id":id,"fee_samples":fees,"fee_for_message":fee_check,"blockhash_context":block["context"],"leader_epoch":leaders.epoch.to_string(),"tip_accounts":tip_addresses,"tip_recipient":chosen_tip.map(|p|p.to_string())})).await?;
        self.policy = next;
        let result = self.routes.send(config, assignment.config.route, &tx).await;
        let utc = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let (status, evidence) = match result {
            SendResult::Accepted => (
                "ACCEPTED",
                json!({"transport_acknowledged":true,"landing_proven":false}),
            ),
            SendResult::Unknown(category) => (
                "UNKNOWN",
                json!({"error_category":category,"landing_proven":false}),
            ),
            SendResult::Rejected(reason) => {
                let evidence = ResolutionEvidence {
                    route_rejection: Some(reason),
                    ..Default::default()
                };
                let resolution = resolver::resolve(&canary, &evidence, &utc);
                self.store
                    .save_resolution(
                        &resolution.canary,
                        resolution.finalized,
                        &utc,
                        &json!({"reason":resolution.reason,"evidence":evidence}),
                    )
                    .await?;
                ("REJECTED", json!({"reason":reason}))
            }
        };
        self.store
            .complete_send(&id, status, &utc, &evidence)
            .await?;
        Ok(
            json!({"status":status,"canary_id":id,"route":assignment.config.route,"reserved_lamports":worst_cost.to_string()}),
        )
    }
}
