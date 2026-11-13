/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Token usage preservation, admission boundary accounting, checkpoint HMAC
//! verification, receipt-vs-proxy reconciliation, and standardized pricing.

use crate::contract::{
    ContractError, JsonObject, canonical_json_string, exact_object, identifier, nonnegative_u64,
    sha256_hex,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub mod gateway;
pub use gateway::*;

pub const METERING_CHECKPOINT_LIMIT_BYTES: usize = 64 * 1024;
pub const COST_USD_TOLERANCE: f64 = 1e-9;

/// Exact 6-counter normalized token usage.
///
/// Each counter is preserved as reported by the provider or receipt; `total_tokens`
/// is never synthesized or overwritten by summing the other five counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

impl NormalizedUsage {
    pub fn decode(value: &Value, where_label: &str) -> Result<Self, ContractError> {
        let obj = exact_object(
            value,
            where_label,
            &[
                "inputTokens",
                "outputTokens",
                "cacheReadTokens",
                "cacheWriteTokens",
                "reasoningTokens",
                "totalTokens",
            ],
            &[],
        )?;
        Ok(Self {
            input_tokens: nonnegative_u64(
                obj.get("inputTokens").unwrap(),
                &format!("{where_label}.inputTokens"),
            )?,
            output_tokens: nonnegative_u64(
                obj.get("outputTokens").unwrap(),
                &format!("{where_label}.outputTokens"),
            )?,
            cache_read_tokens: nonnegative_u64(
                obj.get("cacheReadTokens").unwrap(),
                &format!("{where_label}.cacheReadTokens"),
            )?,
            cache_write_tokens: nonnegative_u64(
                obj.get("cacheWriteTokens").unwrap(),
                &format!("{where_label}.cacheWriteTokens"),
            )?,
            reasoning_tokens: nonnegative_u64(
                obj.get("reasoningTokens").unwrap(),
                &format!("{where_label}.reasoningTokens"),
            )?,
            total_tokens: nonnegative_u64(
                obj.get("totalTokens").unwrap(),
                &format!("{where_label}.totalTokens"),
            )?,
        })
    }

    /// Accumulates another `NormalizedUsage` record counter-by-counter without
    /// synthesizing `total_tokens` from the component fields.
    pub fn accumulate_exact(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }

    pub fn max_by_counter(&self, other: &Self) -> Self {
        Self {
            input_tokens: self.input_tokens.max(other.input_tokens),
            output_tokens: self.output_tokens.max(other.output_tokens),
            cache_read_tokens: self.cache_read_tokens.max(other.cache_read_tokens),
            cache_write_tokens: self.cache_write_tokens.max(other.cache_write_tokens),
            reasoning_tokens: self.reasoning_tokens.max(other.reasoning_tokens),
            total_tokens: self.total_tokens.max(other.total_tokens),
        }
    }

    pub fn dominates(&self, other: &Self) -> bool {
        self.input_tokens >= other.input_tokens
            && self.output_tokens >= other.output_tokens
            && self.cache_read_tokens >= other.cache_read_tokens
            && self.cache_write_tokens >= other.cache_write_tokens
            && self.reasoning_tokens >= other.reasoning_tokens
            && self.total_tokens >= other.total_tokens
    }

    pub fn billing_counters_eq(&self, other: &Self) -> bool {
        self.input_tokens == other.input_tokens
            && self.output_tokens == other.output_tokens
            && self.cache_read_tokens == other.cache_read_tokens
            && self.cache_write_tokens == other.cache_write_tokens
            && self.reasoning_tokens == other.reasoning_tokens
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeteringState {
    Complete,
    LowerBound,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenBasis {
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "lower-bound")]
    LowerBound,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingRates {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_write_per_million: f64,
}

/// Published DeepSeek V4 Flash prices in USD per million tokens (off-peak band).
pub const DEEPSEEK_V4_FLASH_COST: PricingRates = PricingRates {
    input_per_million: 0.22,
    output_per_million: 0.66,
    cache_read_per_million: 0.007,
    cache_write_per_million: 0.22,
};

pub fn deepseek_cost_usd(usage: &NormalizedUsage) -> f64 {
    price_usage_usd(usage, &DEEPSEEK_V4_FLASH_COST)
}

pub fn price_usage_usd(usage: &NormalizedUsage, rates: &PricingRates) -> f64 {
    let cached = usage
        .cache_read_tokens
        .saturating_add(usage.cache_write_tokens);
    let uncached = usage.input_tokens.saturating_sub(cached);
    ((uncached as f64) * rates.input_per_million
        + (usage.cache_read_tokens as f64) * rates.cache_read_per_million
        + (usage.cache_write_tokens as f64) * rates.cache_write_per_million
        + (usage.output_tokens as f64) * rates.output_per_million)
        / 1_000_000.0
}

pub fn costs_match_within_tolerance(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() <= COST_USD_TOLERANCE,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderMeteringCounts {
    pub usage: Option<NormalizedUsage>,
    pub settled: bool,
    pub requests: u64,
    pub in_flight_requests: u64,
    pub admitted_requests: u64,
    pub usage_requests: u64,
    #[serde(default)]
    pub unknown_requests: u64,
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_usage: BTreeMap<String, NormalizedUsage>,
    pub tool_names: Vec<String>,
}

impl ProviderMeteringCounts {
    pub fn validate(&self) -> bool {
        if self.settled && self.in_flight_requests > 0 {
            return false;
        }
        let Some(accounted) = self
            .in_flight_requests
            .checked_add(self.admitted_requests)
            .and_then(|n| n.checked_add(self.unknown_requests))
        else {
            return false;
        };
        if accounted > self.requests || self.usage_requests > self.admitted_requests {
            return false;
        }
        if self.usage_requests > 0 && self.usage.is_none() {
            return false;
        }
        if self.admitted_requests == 0 && (self.usage.is_some() || !self.model_usage.is_empty()) {
            return false;
        }
        if !self.model_usage.is_empty() {
            let Some(total) = &self.usage else {
                return false;
            };
            let known_models: BTreeSet<&str> = self.models.iter().map(String::as_str).collect();
            let mut accumulated = NormalizedUsage::default();
            for (model, usage) in &self.model_usage {
                if model.is_empty() || !known_models.contains(model.as_str()) {
                    return false;
                }
                accumulated.accumulate_exact(usage);
            }
            if !total.dominates(&accumulated) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedMetering {
    pub settled_requests: u64,
    pub missing_usage_requests: u64,
    pub usage_complete: bool,
    pub cost_usd: Option<f64>,
    pub token_basis: TokenBasis,
    pub metering_state: MeteringState,
}

pub fn default_model_pricing(model: &str) -> Option<PricingRates> {
    (model == "deepseek-v4-flash").then_some(DEEPSEEK_V4_FLASH_COST)
}

pub fn derive_metering_with_pricing<F>(
    counts: &ProviderMeteringCounts,
    rates_for_model: F,
) -> DerivedMetering
where
    F: Fn(&str) -> Option<PricingRates>,
{
    let usage_complete = counts.validate()
        && counts.settled
        && counts.in_flight_requests == 0
        && counts.unknown_requests == 0
        && counts.admitted_requests > 0
        && counts.usage_requests == counts.admitted_requests
        && counts.usage.is_some();
    let cost_usd = match (&counts.usage, usage_complete) {
        (Some(total), true) => {
            if !counts.model_usage.is_empty() {
                let mut sum = NormalizedUsage::default();
                let mut total_cost = 0.0f64;
                let mut all_priced = true;
                for (model, usage) in &counts.model_usage {
                    sum.accumulate_exact(usage);
                    if let Some(rates) = rates_for_model(model) {
                        total_cost += price_usage_usd(usage, &rates);
                    } else {
                        all_priced = false;
                    }
                }
                if all_priced && sum == *total && total_cost.is_finite() {
                    Some(total_cost)
                } else {
                    None
                }
            } else if counts.models.len() == 1 {
                rates_for_model(&counts.models[0]).map(|rates| price_usage_usd(total, &rates))
            } else {
                None
            }
        }
        _ => None,
    };
    let metering_state = match (&counts.usage, usage_complete) {
        (Some(_), true) => MeteringState::Complete,
        (Some(_), false) => MeteringState::LowerBound,
        (None, _) => MeteringState::Unknown,
    };
    DerivedMetering {
        settled_requests: counts.requests.saturating_sub(counts.in_flight_requests),
        missing_usage_requests: counts
            .admitted_requests
            .saturating_sub(counts.usage_requests)
            .saturating_add(counts.unknown_requests),
        usage_complete,
        cost_usd,
        token_basis: if usage_complete {
            TokenBasis::Complete
        } else {
            TokenBasis::LowerBound
        },
        metering_state,
    }
}

pub fn derive_metering(counts: &ProviderMeteringCounts) -> DerivedMetering {
    derive_metering_with_pricing(counts, default_model_pricing)
}

fn is_zero_settled_proxy(p: &ProviderMeteringCounts) -> bool {
    p.validate()
        && p.settled
        && p.in_flight_requests == 0
        && p.unknown_requests == 0
        && p.admitted_requests == 0
        && p.usage.is_none()
}

/// Reconciles Host-reported usage with proxy-recovered usage without double-counting.
///
/// - When both Host receipts and proxy counts are settled and complete, their billing counters
///   must agree (and `total_tokens` must either match verbatim or equal `input + output` when
///   Host receipts only captured 5 counters); otherwise reconciliation reports
///   `MeteringState::LowerBound` with the counter-wise maximum.
/// - When the Host died before settling receipts, the verified proxy checkpoint provides
///   either `Complete` (if the proxy settled with all admitted requests accounted for) or
///   `LowerBound` usage.
pub fn reconcile_host_and_proxy_usage(
    host_usage: Option<&NormalizedUsage>,
    host_settled: bool,
    proxy_counts: Option<&ProviderMeteringCounts>,
) -> (Option<NormalizedUsage>, MeteringState) {
    match (host_usage, host_settled, proxy_counts) {
        (Some(h), true, Some(p)) => {
            if is_zero_settled_proxy(p) && *h == NormalizedUsage::default() {
                return (Some(h.clone()), MeteringState::Complete);
            }
            let derived = derive_metering(p);
            if derived.usage_complete
                && let Some(pu) = p.usage.as_ref()
                && pu.billing_counters_eq(h)
                && (pu.total_tokens == h.total_tokens
                    || h.total_tokens == h.input_tokens.saturating_add(h.output_tokens))
            {
                (Some(pu.clone()), MeteringState::Complete)
            } else {
                let merged = p
                    .usage
                    .as_ref()
                    .map_or_else(|| h.clone(), |pu| h.max_by_counter(pu));
                (Some(merged), MeteringState::LowerBound)
            }
        }
        (Some(h), true, None) => (Some(h.clone()), MeteringState::Complete),
        (Some(h), false, Some(p)) => {
            let derived = derive_metering(p);
            if derived.usage_complete && p.usage.as_ref().is_some_and(|pu| pu.dominates(h)) {
                (p.usage.clone(), MeteringState::Complete)
            } else {
                let merged = p
                    .usage
                    .as_ref()
                    .map_or_else(|| h.clone(), |pu| h.max_by_counter(pu));
                (Some(merged), MeteringState::LowerBound)
            }
        }
        (Some(h), false, None) => (Some(h.clone()), MeteringState::LowerBound),
        (None, true, Some(p)) if is_zero_settled_proxy(p) => (None, MeteringState::Unknown),
        (None, _, Some(p)) => {
            let derived = derive_metering(p);
            (p.usage.clone(), derived.metering_state)
        }
        (None, _, None) => (None, MeteringState::Unknown),
    }
}

/// Computes RFC 2104 HMAC-SHA256 in hex.
pub fn hmac_sha256_hex(secret: &[u8], message: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if secret.len() > BLOCK_SIZE {
        let digest = Sha256::digest(secret);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..secret.len()].copy_from_slice(secret);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    hex::encode(outer.finalize())
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn without_mac(record: &JsonObject) -> JsonObject {
    let mut unsigned = Map::with_capacity(record.len());
    for (k, v) in record {
        if k != "mac" {
            unsigned.insert(k.clone(), v.clone());
        }
    }
    unsigned
}

pub fn sign_metering_checkpoint(body: &JsonObject, secret: &str) -> JsonObject {
    let unsigned = without_mac(body);
    let canonical = canonical_json_string(&Value::Object(unsigned));
    let mac = hmac_sha256_hex(secret.as_bytes(), canonical.as_bytes());
    let mut signed = match serde_json::from_str::<Value>(&canonical) {
        Ok(Value::Object(map)) => map,
        _ => unreachable!("canonical_json_string of an object is always an object"),
    };
    signed.insert("mac".into(), Value::String(mac));
    signed
}

pub fn metering_checkpoint_mac_matches(record: &JsonObject, secret: &str) -> bool {
    let Some(mac) = record.get("mac").and_then(Value::as_str) else {
        return false;
    };
    if mac.len() != 64 || !mac.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return false;
    }
    let unsigned = without_mac(record);
    let canonical = canonical_json_string(&Value::Object(unsigned));
    let expected = hmac_sha256_hex(secret.as_bytes(), canonical.as_bytes());
    constant_time_eq(expected.as_bytes(), mac.as_bytes())
}

/// Reads a bounded regular file while refusing symlinks (`O_NOFOLLOW` on unix)
/// and oversized or empty files. Returns `Ok(None)` only when the file does not exist.
pub fn read_bounded_regular_file(
    path: &Path,
    limit_bytes: usize,
) -> Result<Option<Vec<u8>>, ContractError> {
    use std::io::Read;
    let symlink_meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(ContractError::Invalid(format!(
                "failed to stat {}: {err}",
                path.display()
            )));
        }
    };
    if !symlink_meta.file_type().is_file() {
        return Err(ContractError::Invalid(format!(
            "{} must be a regular file",
            path.display()
        )));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|err| {
        ContractError::Invalid(format!("failed to open {}: {err}", path.display()))
    })?;
    let meta = file.metadata().map_err(|err| {
        ContractError::Invalid(format!("failed to stat {}: {err}", path.display()))
    })?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > (limit_bytes as u64) {
        return Err(ContractError::Invalid(format!(
            "{} has invalid file size ({} bytes)",
            path.display(),
            meta.len()
        )));
    }
    let size = meta.len() as usize;
    let mut buf = Vec::with_capacity(size);
    file.read_to_end(&mut buf).map_err(|err| {
        ContractError::Invalid(format!("failed to read {}: {err}", path.display()))
    })?;
    if buf.len() != size {
        return Err(ContractError::Invalid(format!(
            "{} size changed while reading",
            path.display()
        )));
    }
    Ok(Some(buf))
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoveredExternalMetering {
    pub usage: Option<NormalizedUsage>,
    pub cost_usd: Option<f64>,
    pub admitted_requests: u64,
    pub metering_state: MeteringState,
    pub counts: ProviderMeteringCounts,
    pub artifact: JsonObject,
}

pub fn recover_external_metering_checkpoint(
    trial_path: &Path,
    profile: &str,
    metering_secret: &str,
) -> Result<Option<RecoveredExternalMetering>, ContractError> {
    let profile = identifier(
        &Value::String(profile.to_owned()),
        "external metering profile",
    )?;
    let rel_path = format!("agent/{profile}.provider-usage.json");
    let full_path = trial_path.join(&rel_path);
    let Some(bytes) = read_bounded_regular_file(&full_path, METERING_CHECKPOINT_LIMIT_BYTES)?
    else {
        return Ok(None);
    };
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|err| {
        ContractError::Invalid(format!("invalid external metering checkpoint JSON: {err}"))
    })?;
    let checkpoint = exact_object(
        &parsed,
        "external metering checkpoint",
        &[
            "schemaVersion",
            "profile",
            "usage",
            "settled",
            "requests",
            "inFlightRequests",
            "admittedRequests",
            "usageRequests",
            "models",
            "toolNames",
            "mac",
        ],
        &["unknownRequests", "modelUsage"],
    )?;
    if checkpoint.get("schemaVersion").and_then(Value::as_str)
        != Some("maka.external_provider_usage.v2")
    {
        return Err(ContractError::Invalid(
            "unsupported external metering checkpoint schemaVersion".into(),
        ));
    }
    if checkpoint.get("profile").and_then(Value::as_str) != Some(profile.as_str()) {
        return Err(ContractError::Invalid(
            "external metering checkpoint profile mismatch".into(),
        ));
    }
    if !metering_checkpoint_mac_matches(checkpoint, metering_secret) {
        return Err(ContractError::Invalid(
            "external metering checkpoint MAC verification failed".into(),
        ));
    }
    let usage = match checkpoint.get("usage").unwrap() {
        Value::Null => None,
        u => Some(NormalizedUsage::decode(u, "external usage")?),
    };
    let settled = checkpoint
        .get("settled")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ContractError::Invalid("external metering settled must be boolean".into())
        })?;
    let requests = nonnegative_u64(checkpoint.get("requests").unwrap(), "requests")?;
    let in_flight_requests = nonnegative_u64(
        checkpoint.get("inFlightRequests").unwrap(),
        "inFlightRequests",
    )?;
    let admitted_requests = nonnegative_u64(
        checkpoint.get("admittedRequests").unwrap(),
        "admittedRequests",
    )?;
    let usage_requests =
        nonnegative_u64(checkpoint.get("usageRequests").unwrap(), "usageRequests")?;
    let unknown_requests = match checkpoint.get("unknownRequests") {
        None => 0,
        Some(v) => nonnegative_u64(v, "unknownRequests")?,
    };
    let models = checkpoint
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| ContractError::Invalid("external metering models must be an array".into()))?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_owned).ok_or_else(|| {
                ContractError::Invalid("external metering model must be string".into())
            })
        })
        .collect::<Result<Vec<String>, ContractError>>()?;
    let model_usage = match checkpoint.get("modelUsage") {
        None => BTreeMap::new(),
        Some(val) => {
            let obj = val.as_object().ok_or_else(|| {
                ContractError::Invalid("external metering modelUsage must be an object".into())
            })?;
            let mut decoded = BTreeMap::new();
            for (model, u_val) in obj {
                decoded.insert(
                    model.clone(),
                    NormalizedUsage::decode(u_val, &format!("external modelUsage.{model}"))?,
                );
            }
            decoded
        }
    };
    let tool_names = checkpoint
        .get("toolNames")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ContractError::Invalid("external metering toolNames must be an array".into())
        })?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_owned).ok_or_else(|| {
                ContractError::Invalid("external metering toolName must be string".into())
            })
        })
        .collect::<Result<Vec<String>, ContractError>>()?;

    let counts = ProviderMeteringCounts {
        usage: usage.clone(),
        settled,
        requests,
        in_flight_requests,
        admitted_requests,
        usage_requests,
        unknown_requests,
        models: models.clone(),
        model_usage,
        tool_names: tool_names.clone(),
    };
    if !counts.validate() {
        return Err(ContractError::Invalid(
            "external metering checkpoint failed invariant validation".into(),
        ));
    }
    let derived = derive_metering(&counts);
    let mut artifact = Map::new();
    artifact.insert(
        "kind".into(),
        Value::String("provider-metering-recovery".into()),
    );
    artifact.insert("profile".into(), Value::String(profile));
    artifact.insert("path".into(), Value::String(rel_path));
    artifact.insert("bytes".into(), Value::from(bytes.len() as u64));
    artifact.insert("sha256".into(), Value::String(sha256_hex(&bytes)));
    artifact.insert("requests".into(), Value::from(counts.requests));
    artifact.insert(
        "settledRequests".into(),
        Value::from(derived.settled_requests),
    );
    artifact.insert(
        "inFlightRequests".into(),
        Value::from(counts.in_flight_requests),
    );
    artifact.insert(
        "admittedRequests".into(),
        Value::from(counts.admitted_requests),
    );
    artifact.insert("usageRequests".into(), Value::from(counts.usage_requests));
    artifact.insert(
        "unknownRequests".into(),
        Value::from(counts.unknown_requests),
    );
    artifact.insert(
        "missingUsageRequests".into(),
        Value::from(derived.missing_usage_requests),
    );
    artifact.insert("usageComplete".into(), Value::Bool(derived.usage_complete));
    artifact.insert(
        "tokenBasis".into(),
        serde_json::to_value(derived.token_basis).map_err(|err| {
            ContractError::Invalid(format!("failed to serialize tokenBasis: {err}"))
        })?,
    );
    artifact.insert(
        "models".into(),
        Value::Array(models.into_iter().map(Value::String).collect()),
    );
    artifact.insert(
        "toolNames".into(),
        Value::Array(tool_names.into_iter().map(Value::String).collect()),
    );

    Ok(Some(RecoveredExternalMetering {
        usage,
        cost_usd: derived.cost_usd,
        admitted_requests: counts.admitted_requests,
        metering_state: derived.metering_state,
        counts,
        artifact,
    }))
}
