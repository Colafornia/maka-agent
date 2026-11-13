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

use maka_eval::measurement::{
    MeteringState, NormalizedUsage, ProviderMeteringCounts, derive_metering,
    metering_checkpoint_mac_matches, reconcile_host_and_proxy_usage,
    recover_external_metering_checkpoint, sign_metering_checkpoint,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

#[test]
fn metering_reconciliation_pricing_and_zero_request_settlement() {
    let wire_usage = NormalizedUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 20,
        cache_write_tokens: 0,
        reasoning_tokens: 10,
        total_tokens: 999,
    };
    let host_5_counter = NormalizedUsage {
        total_tokens: 150, // input + output
        ..wire_usage.clone()
    };

    let mut proxy_counts = ProviderMeteringCounts {
        usage: Some(wire_usage.clone()),
        settled: true,
        requests: 1,
        in_flight_requests: 0,
        admitted_requests: 1,
        usage_requests: 1,
        unknown_requests: 0,
        models: vec!["deepseek-v4-flash".into()],
        model_usage: BTreeMap::from([("deepseek-v4-flash".into(), wire_usage.clone())]),
        tool_names: Vec::new(),
    };

    let derived = derive_metering(&proxy_counts);
    assert!(derived.usage_complete);
    assert_eq!(derived.metering_state, MeteringState::Complete);
    assert!(derived.cost_usd.is_some());

    // Reconciling a 5-counter Host receipt against wire-exact proxy usage stays Complete
    // and preserves the proxy's wire total_tokens.
    let (reconciled, state) =
        reconcile_host_and_proxy_usage(Some(&host_5_counter), true, Some(&proxy_counts));
    assert_eq!(state, MeteringState::Complete);
    assert_eq!(reconciled.as_ref().unwrap().total_tokens, 999);

    // Non-DeepSeek models do not fabricate DeepSeek V4 Flash pricing.
    let other_model_counts = ProviderMeteringCounts {
        models: vec!["claude-sonnet".into()],
        model_usage: BTreeMap::from([("claude-sonnet".into(), wire_usage.clone())]),
        ..proxy_counts.clone()
    };
    let other_derived = derive_metering(&other_model_counts);
    assert!(other_derived.usage_complete);
    assert_eq!(other_derived.cost_usd, None);

    // Even when Host thinks it settled completely, unknown proxy requests downgrade to LowerBound.
    proxy_counts.requests += 1;
    proxy_counts.unknown_requests = 1;
    let (unknown_reconciled, unknown_state) =
        reconcile_host_and_proxy_usage(Some(&host_5_counter), true, Some(&proxy_counts));
    assert_eq!(unknown_state, MeteringState::LowerBound);
    assert_eq!(unknown_reconciled.unwrap().total_tokens, 999);

    // Zero-request settled run with zero Host usage stays Complete.
    let zero_proxy = ProviderMeteringCounts {
        usage: None,
        settled: true,
        requests: 0,
        in_flight_requests: 0,
        admitted_requests: 0,
        usage_requests: 0,
        unknown_requests: 0,
        models: Vec::new(),
        model_usage: BTreeMap::new(),
        tool_names: Vec::new(),
    };
    let (zero_usage, zero_state) =
        reconcile_host_and_proxy_usage(Some(&NormalizedUsage::default()), true, Some(&zero_proxy));
    assert_eq!(zero_state, MeteringState::Complete);
    assert_eq!(zero_usage, Some(NormalizedUsage::default()));
}

#[test]
fn checkpoint_hmac_is_order_independent_and_distinguishes_missing_vs_corrupt() {
    let secret = "0123456789abcdef0123456789abcdef";
    let body: Map<String, Value> = serde_json::from_value(json!({
        "schemaVersion": "maka.external_provider_usage.v2",
        "profile": "maka",
        "usage": {
            "inputTokens": 50,
            "outputTokens": 10,
            "cacheReadTokens": 0,
            "cacheWriteTokens": 0,
            "reasoningTokens": 0,
            "totalTokens": 60
        },
        "settled": true,
        "requests": 1,
        "inFlightRequests": 0,
        "admittedRequests": 1,
        "usageRequests": 1,
        "unknownRequests": 0,
        "models": ["deepseek-v4-flash"],
        "toolNames": []
    }))
    .unwrap();
    let signed = sign_metering_checkpoint(&body, secret);
    assert!(metering_checkpoint_mac_matches(&signed, secret));

    // Reordering keys and placing "mac" first must still validate identically.
    let mut reordered = Map::new();
    reordered.insert("mac".into(), signed.get("mac").unwrap().clone());
    for (k, v) in signed.iter().rev() {
        if k != "mac" {
            reordered.insert(k.clone(), v.clone());
        }
    }
    assert!(metering_checkpoint_mac_matches(&reordered, secret));

    let temp = TempDir::new().unwrap();
    // Missing checkpoint => Ok(None).
    assert_eq!(
        recover_external_metering_checkpoint(temp.path(), "maka", secret).unwrap(),
        None
    );
    // Traversal profile => Err.
    assert!(recover_external_metering_checkpoint(temp.path(), "../escape", secret).is_err());

    // Valid file => Ok(Some(_)); tampered MAC => Err.
    let agent_dir = temp.path().join("agent");
    fs::create_dir_all(&agent_dir).unwrap();
    let cp_path = agent_dir.join("maka.provider-usage.json");
    fs::write(
        &cp_path,
        serde_json::to_vec(&Value::Object(reordered)).unwrap(),
    )
    .unwrap();
    let recovered = recover_external_metering_checkpoint(temp.path(), "maka", secret)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.metering_state, MeteringState::Complete);
    assert!(recover_external_metering_checkpoint(temp.path(), "maka", "wrong-secret").is_err());
}
