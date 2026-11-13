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

//! Local loopback HTTP metering and admission gateway for Maka Host and external subjects.
//!
//! Binds `127.0.0.1:0`, authenticates per-attempt `<GATEWAY_CREDENTIAL>`, forwards request
//! payloads untouched using the real provider credential, parses streaming SSE and JSON
//! usage/admission facts, writes atomic HMAC-signed checkpoints, and enforces a synchronous
//! `stop_accepting()` cutoff while draining in-flight requests.

use crate::contract::{JsonObject, identifier};
use crate::measurement::{
    NormalizedUsage, ProviderMeteringCounts, constant_time_eq, sign_metering_checkpoint,
};
use http_body_util::{BodyExt, Full, LengthLimitError, Limited, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;

pub const PROVIDER_USAGE_CHECKPOINT_SCHEMA: &str = "maka.external_provider_usage.v2";
pub const DEFAULT_GATEWAY_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_GATEWAY_REQUEST_BODY_BYTES: usize = 64 * 1024 * 1024;

type BoxBody = http_body_util::combinators::BoxBody<Bytes, Infallible>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservedRequestMetadata {
    pub model: Option<String>,
    pub tool_names: Vec<String>,
}

/// Inspects an OpenAI or Anthropic JSON request payload to observe the requested model and
/// exposed tool names without mutating or re-serializing the request body.
pub fn inspect_request_body(body: &[u8]) -> ObservedRequestMetadata {
    if body.is_empty() {
        return ObservedRequestMetadata::default();
    }
    let Ok(Value::Object(obj)) = serde_json::from_slice::<Value>(body) else {
        return ObservedRequestMetadata::default();
    };
    let model = obj
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let tool_names = obj
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| tools.iter().flat_map(extract_tool_name).collect())
        .unwrap_or_default();
    ObservedRequestMetadata { model, tool_names }
}

fn extract_tool_name(tool: &Value) -> Vec<String> {
    let Some(obj) = tool.as_object() else {
        return Vec::new();
    };
    if let Some(name) = obj.get("name").and_then(Value::as_str) {
        return vec![name.to_owned()];
    }
    if let Some(name) = obj
        .get("function")
        .and_then(Value::as_object)
        .and_then(|f| f.get("name"))
        .and_then(Value::as_str)
    {
        return vec![name.to_owned()];
    }
    if let Some(t) = obj.get("type").and_then(Value::as_str) {
        return vec![t.to_owned()];
    }
    Vec::new()
}

/// Determines whether a parsed provider JSON/SSE event proves the upstream provider
/// admitted an inference call.
pub fn is_inference_admission_event(event: &Map<String, Value>) -> bool {
    let object = event.get("object").and_then(Value::as_str);
    if matches!(
        object,
        Some("chat.completion") | Some("chat.completion.chunk")
    ) {
        return true;
    }
    let event_type = event.get("type").and_then(Value::as_str);
    if let Some(et) = event_type
        && matches!(
            et,
            "response.created"
                | "response.in_progress"
                | "response.output_item.added"
                | "response.content_part.added"
                | "response.output_text.delta"
                | "response.reasoning_text.delta"
                | "response.function_call_arguments.delta"
                | "response.completed"
                | "response.incomplete"
                | "response.failed"
        )
    {
        return true;
    }
    if object == Some("response")
        && event
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && (event.get("output").is_some_and(Value::is_array)
            || event.get("usage").is_some_and(Value::is_object))
    {
        return true;
    }
    match event_type {
        Some("message_start") => event.get("message").is_some_and(Value::is_object),
        Some("message_delta") => event.get("usage").is_some_and(Value::is_object),
        Some("message") => {
            event
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
                && (event.get("content").is_some_and(Value::is_array)
                    || event.get("usage").is_some_and(Value::is_object))
        }
        _ => false,
    }
}

fn extract_usage_container(event: &Map<String, Value>) -> &Map<String, Value> {
    if let Some(resp) = event.get("response").and_then(Value::as_object) {
        return resp;
    }
    if event.get("type").and_then(Value::as_str) == Some("message_start")
        && let Some(msg) = event.get("message").and_then(Value::as_object)
    {
        return msg;
    }
    event
}

fn extract_usage_object(event: &Map<String, Value>) -> Option<&Map<String, Value>> {
    let container = extract_usage_container(event);
    container
        .get("usage")
        .and_then(Value::as_object)
        .or_else(|| event.get("usage").and_then(Value::as_object))
}

fn extract_model_name(event: &Map<String, Value>) -> Option<String> {
    let container = extract_usage_container(event);
    container
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            event
                .get("model")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .map(str::to_owned)
}

/// Extracts a 6-counter `NormalizedUsage` snapshot from an OpenAI-compatible or Anthropic
/// event/response.
///
/// Requires valid non-negative integer input and output token counts (returns `None` for
/// empty `{}` or incomplete usage objects) and preserves an explicit `total_tokens` value
/// verbatim, including `0`. For Anthropic Messages payloads, normalizes `input_tokens` to
/// include uncached + cache read + cache write tokens to match `NormalizedUsage` billing.
pub fn usage_from_event(event: &Map<String, Value>) -> Option<NormalizedUsage> {
    let raw = extract_usage_object(event)?;
    let is_anthropic = matches!(
        event.get("type").and_then(Value::as_str),
        Some("message" | "message_start" | "message_delta")
    );

    let input = lookup_u64_field(raw, &["input_tokens", "prompt_tokens"]).ok()??;
    let output = lookup_u64_field(raw, &["output_tokens", "completion_tokens"]).ok()??;
    let input_details = raw
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .or_else(|| raw.get("prompt_tokens_details").and_then(Value::as_object));
    let output_details = raw
        .get("output_tokens_details")
        .and_then(Value::as_object)
        .or_else(|| {
            raw.get("completion_tokens_details")
                .and_then(Value::as_object)
        });

    let cache_read = lookup_u64_field(raw, &["cache_read_input_tokens", "prompt_cache_hit_tokens"])
        .ok()?
        .or_else(|| {
            input_details.and_then(|d| lookup_u64_field(d, &["cached_tokens"]).ok().flatten())
        })
        .unwrap_or(0);
    let cache_write = lookup_u64_field(
        raw,
        &["cache_creation_input_tokens", "cache_write_input_tokens"],
    )
    .ok()?
    .or_else(|| {
        input_details.and_then(|d| {
            lookup_u64_field(d, &["cache_write_tokens", "cache_creation_tokens"])
                .ok()
                .flatten()
        })
    })
    .unwrap_or(0);
    let reasoning = match output_details {
        Some(d) => lookup_u64_field(d, &["reasoning_tokens", "thinking_tokens"])
            .ok()?
            .unwrap_or(0),
        None => 0,
    };
    let total_input = if is_anthropic {
        input.saturating_add(cache_read).saturating_add(cache_write)
    } else {
        input
    };
    let total_tokens = match lookup_u64_field(raw, &["total_tokens"]).ok()? {
        Some(reported) => reported,
        None => total_input.saturating_add(output),
    };
    Some(NormalizedUsage {
        input_tokens: total_input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens,
    })
}

fn lookup_u64_field(obj: &Map<String, Value>, keys: &[&str]) -> Result<Option<u64>, ()> {
    for key in keys {
        match obj.get(*key) {
            None | Some(Value::Null) => continue,
            Some(val) => return val.as_u64().map(Some).ok_or(()),
        }
    }
    Ok(None)
}

fn merge_billing_counters(
    left: Option<NormalizedUsage>,
    right: NormalizedUsage,
) -> NormalizedUsage {
    match left {
        None => right,
        Some(l) => NormalizedUsage {
            input_tokens: l.input_tokens.max(right.input_tokens),
            output_tokens: l.output_tokens.max(right.output_tokens),
            cache_read_tokens: l.cache_read_tokens.max(right.cache_read_tokens),
            cache_write_tokens: l.cache_write_tokens.max(right.cache_write_tokens),
            reasoning_tokens: l.reasoning_tokens.max(right.reasoning_tokens),
            total_tokens: l.total_tokens.max(right.total_tokens),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedResponseUsage {
    pub usage: Option<NormalizedUsage>,
    pub model: Option<String>,
    pub admitted: bool,
    pub settled: bool,
}

const NON_SSE_RESPONSE_BUFFER_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Incremental SSE / JSON response parser for a single model request.
#[derive(Debug, Default)]
pub struct UsageStreamParser {
    buffered: String,
    full_text: String,
    utf8_tail: Vec<u8>,
    current: Option<NormalizedUsage>,
    model: Option<String>,
    explicit_total_tokens: Option<u64>,
    admitted: bool,
    saw_sse_frame: bool,
    saw_terminal_event: bool,
    overflowed: bool,
}

impl UsageStreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: &str) {
        if self.overflowed {
            return;
        }
        self.buffered.push_str(text);
        if !self.saw_sse_frame
            && self.full_text.len().saturating_add(text.len())
                <= NON_SSE_RESPONSE_BUFFER_LIMIT_BYTES
        {
            self.full_text.push_str(text);
        }
        while let Some(newline) = self.buffered.find('\n') {
            if newline > NON_SSE_RESPONSE_BUFFER_LIMIT_BYTES {
                self.buffered.clear();
                self.full_text.clear();
                self.overflowed = true;
                return;
            }
            let line = self.buffered[..newline].to_owned();
            self.buffered.drain(..=newline);
            self.consume_line(&line);
        }
        if self.buffered.len() > NON_SSE_RESPONSE_BUFFER_LIMIT_BYTES {
            self.buffered.clear();
            self.full_text.clear();
            self.overflowed = true;
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let mut owned;
        let mut slice: &[u8] = if self.utf8_tail.is_empty() {
            bytes
        } else {
            owned = std::mem::take(&mut self.utf8_tail);
            owned.extend_from_slice(bytes);
            owned.as_slice()
        };
        while !slice.is_empty() {
            match std::str::from_utf8(slice) {
                Ok(text) => {
                    self.push(text);
                    break;
                }
                Err(err) => {
                    let valid = &slice[..err.valid_up_to()];
                    if !valid.is_empty() {
                        self.push(std::str::from_utf8(valid).expect("validated UTF-8 prefix"));
                    }
                    if let Some(bad_len) = err.error_len() {
                        self.push("\u{FFFD}");
                        slice = &slice[err.valid_up_to() + bad_len..];
                    } else {
                        self.utf8_tail
                            .extend_from_slice(&slice[err.valid_up_to()..]);
                        break;
                    }
                }
            }
        }
    }

    pub fn admitted(&self) -> bool {
        self.admitted
    }

    pub fn finish(&mut self) -> (Option<NormalizedUsage>, bool) {
        let parsed = self.finish_response(true);
        (parsed.usage, parsed.admitted)
    }

    pub fn finish_response(&mut self, transport_complete: bool) -> ParsedResponseUsage {
        if !self.overflowed && !self.utf8_tail.is_empty() {
            let tail_bytes = std::mem::take(&mut self.utf8_tail);
            let lossy = String::from_utf8_lossy(&tail_bytes).into_owned();
            self.push(&lossy);
        }
        if !self.overflowed && !self.buffered.trim().is_empty() {
            let tail = std::mem::take(&mut self.buffered);
            self.consume_line(&tail);
        }
        if !self.overflowed
            && !self.admitted
            && !self.saw_sse_frame
            && !self.full_text.trim().is_empty()
            && let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(self.full_text.trim())
        {
            self.observe_event(&obj);
        }
        ParsedResponseUsage {
            usage: self.current.clone(),
            model: self.model.clone(),
            admitted: self.admitted,
            settled: !self.overflowed
                && transport_complete
                && self.saw_terminal_event
                && self.current.is_some(),
        }
    }

    fn observe_event(&mut self, obj: &Map<String, Value>) {
        self.admitted |= is_inference_admission_event(obj);
        if self.model.is_none() {
            self.model = extract_model_name(obj);
        }
        if let Some(raw) = extract_usage_object(obj)
            && let Ok(Some(explicit)) = lookup_u64_field(raw, &["total_tokens"])
        {
            self.explicit_total_tokens = Some(explicit);
        }
        let parsed_usage = usage_from_event(obj);
        if let Some(usage) = parsed_usage {
            let mut merged = merge_billing_counters(self.current.take(), usage);
            merged.total_tokens = self
                .explicit_total_tokens
                .unwrap_or_else(|| merged.input_tokens.saturating_add(merged.output_tokens));
            self.current = Some(merged);
        } else if obj.get("type").and_then(Value::as_str) == Some("message_delta")
            && let Some(prev) = self.current.as_mut()
            && let Some(raw) = extract_usage_object(obj)
        {
            if let Ok(Some(output)) = lookup_u64_field(raw, &["output_tokens", "completion_tokens"])
            {
                prev.output_tokens = prev.output_tokens.max(output);
            }
            let input_details = raw
                .get("input_tokens_details")
                .and_then(Value::as_object)
                .or_else(|| raw.get("prompt_tokens_details").and_then(Value::as_object));
            let output_details = raw
                .get("output_tokens_details")
                .and_then(Value::as_object)
                .or_else(|| {
                    raw.get("completion_tokens_details")
                        .and_then(Value::as_object)
                });
            if let Ok(Some(cache_read)) =
                lookup_u64_field(raw, &["cache_read_input_tokens", "prompt_cache_hit_tokens"])
            {
                prev.cache_read_tokens = prev.cache_read_tokens.max(cache_read);
            } else if let Some(d) = input_details
                && let Ok(Some(cache_read)) = lookup_u64_field(d, &["cached_tokens"])
            {
                prev.cache_read_tokens = prev.cache_read_tokens.max(cache_read);
            }
            if let Ok(Some(cache_write)) = lookup_u64_field(
                raw,
                &["cache_creation_input_tokens", "cache_write_input_tokens"],
            ) {
                prev.cache_write_tokens = prev.cache_write_tokens.max(cache_write);
            } else if let Some(d) = input_details
                && let Ok(Some(cache_write)) =
                    lookup_u64_field(d, &["cache_write_tokens", "cache_creation_tokens"])
            {
                prev.cache_write_tokens = prev.cache_write_tokens.max(cache_write);
            }
            if let Some(d) = output_details
                && let Ok(Some(reasoning)) =
                    lookup_u64_field(d, &["reasoning_tokens", "thinking_tokens"])
            {
                prev.reasoning_tokens = prev.reasoning_tokens.max(reasoning);
            }
            prev.total_tokens = self
                .explicit_total_tokens
                .unwrap_or_else(|| prev.input_tokens.saturating_add(prev.output_tokens));
        }
        let event_type = obj.get("type").and_then(Value::as_str);
        if matches!(
            event_type,
            Some("response.completed" | "response.incomplete" | "response.failed" | "message_stop")
        ) {
            self.saw_terminal_event = true;
            return;
        }
        if !self.saw_sse_frame {
            let object = obj.get("object").and_then(Value::as_str);
            if object == Some("chat.completion")
                || (object == Some("response") && event_type.is_none())
                || event_type == Some("message")
            {
                self.saw_terminal_event = true;
            }
        }
    }

    fn consume_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.starts_with("data:") || trimmed.starts_with("event:") || trimmed.starts_with(':')
        {
            self.saw_sse_frame = true;
            self.full_text.clear();
        }
        let raw = trimmed
            .strip_prefix("data:")
            .map(str::trim_start)
            .unwrap_or(trimmed);
        if raw.is_empty() {
            return;
        }
        if raw == "[DONE]" {
            self.saw_terminal_event = true;
            return;
        }
        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(raw) else {
            return;
        };
        self.observe_event(&obj);
    }
}

pub fn join_upstream_url(base_url: &str, request_uri: &str) -> Result<String, String> {
    let mut base = Url::parse(base_url).map_err(|e| format!("invalid upstream baseUrl: {e}"))?;
    let req_url = Url::parse(&format!(
        "http://maka-eval.local/{}",
        request_uri.trim_start_matches('/')
    ))
    .map_err(|e| format!("invalid request URI: {e}"))?;
    let base_path = base.path().trim_end_matches('/');
    let req_path = req_url.path().trim_start_matches('/');
    // Avoid duplicating path prefixes (such as `/v1` or `/api/v1`) when `base_url`
    // already carries that prefix and the client sends the full prefixed path.
    let combined_path = if !base_path.is_empty()
        && (req_url.path() == base_path || req_url.path().starts_with(&format!("{base_path}/")))
    {
        req_url.path().to_owned()
    } else if req_path.is_empty() {
        base_path.to_owned()
    } else {
        format!("{base_path}/{req_path}")
    };
    base.set_path(&combined_path);
    match (base.query().map(str::to_owned), req_url.query()) {
        (Some(bq), Some(rq)) if !bq.is_empty() && !rq.is_empty() => {
            base.set_query(Some(&format!("{bq}&{rq}")));
        }
        (Some(bq), _) if !bq.is_empty() => {
            base.set_query(Some(&bq));
        }
        (_, Some(rq)) if !rq.is_empty() => {
            base.set_query(Some(rq));
        }
        _ => base.set_query(None),
    }
    Ok(base.to_string())
}

fn rewrite_url_origin(target_url: &str, origin_override: &str) -> Result<String, String> {
    let target = Url::parse(target_url).map_err(|e| format!("invalid target URL: {e}"))?;
    let mut dest =
        Url::parse(origin_override).map_err(|e| format!("invalid origin override: {e}"))?;
    dest.set_path(target.path());
    dest.set_query(target.query());
    dest.set_fragment(None);
    Ok(dest.to_string())
}

pub const UPSTREAM_TARGET_HEADER: &str = "x-maka-eval-upstream-target";

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub upstream_base_url: String,
    pub transport_origin_override: Option<String>,
    pub upstream_api_key: String,
    pub profile: String,
    pub gateway_credential: String,
    pub checkpoint_path: Option<PathBuf>,
    pub checkpoint_secret: String,
    pub drain_timeout: Duration,
}

#[derive(Debug, Default)]
struct GatewayCounters {
    total_usage: NormalizedUsage,
    has_usage: bool,
    requests: u64,
    in_flight_requests: u64,
    admitted_requests: u64,
    usage_requests: u64,
    unknown_requests: u64,
    settled: bool,
    models: BTreeSet<String>,
    model_usage: BTreeMap<String, NormalizedUsage>,
    tool_names: BTreeSet<String>,
}

impl GatewayCounters {
    fn snapshot(&self) -> ProviderMeteringCounts {
        ProviderMeteringCounts {
            usage: if self.has_usage {
                Some(self.total_usage.clone())
            } else {
                None
            },
            settled: self.settled,
            requests: self.requests,
            in_flight_requests: self.in_flight_requests,
            admitted_requests: self.admitted_requests,
            usage_requests: self.usage_requests,
            unknown_requests: self.unknown_requests,
            models: self.models.iter().cloned().collect(),
            model_usage: self.model_usage.clone(),
            tool_names: self.tool_names.iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayPersistenceError {
    pub message: String,
    pub counts: Box<ProviderMeteringCounts>,
}

impl std::fmt::Display for GatewayPersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "metering checkpoint persistence failed: {}",
            self.message
        )
    }
}

impl std::error::Error for GatewayPersistenceError {}

struct SharedGatewayState {
    config: GatewayConfig,
    client: reqwest::Client,
    accepting: AtomicBool,
    counters: Mutex<GatewayCounters>,
    checkpoint_lock: Mutex<()>,
    checkpoint_error: std::sync::Mutex<Option<String>>,
    in_flight_zero: Notify,
    shutdown: CancellationToken,
}

impl SharedGatewayState {
    fn record_persistence_error(&self, err: &str) {
        self.accepting.store(false, Ordering::SeqCst);
        let mut slot = self
            .checkpoint_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(err.to_owned());
        }
    }

    fn persistence_error(&self) -> Option<String> {
        self.checkpoint_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    async fn persist_checkpoint(&self) -> Result<(), String> {
        let Some(path) = self.config.checkpoint_path.clone() else {
            return Ok(());
        };
        let _guard = self.checkpoint_lock.lock().await;
        let snap = self.counters.lock().await.snapshot();
        let bytes_res = (|| -> Result<Vec<u8>, String> {
            let Value::Object(mut body) = serde_json::to_value(&snap)
                .map_err(|e| format!("failed to serialize metering checkpoint: {e}"))?
            else {
                return Err("metering checkpoint did not serialize as JSON object".into());
            };
            body.insert(
                "schemaVersion".into(),
                Value::String(PROVIDER_USAGE_CHECKPOINT_SCHEMA.into()),
            );
            body.insert("profile".into(), Value::String(self.config.profile.clone()));
            let signed: JsonObject =
                sign_metering_checkpoint(&body, &self.config.checkpoint_secret);
            let mut bytes = serde_json::to_vec(&Value::Object(signed))
                .map_err(|e| format!("failed to encode metering checkpoint: {e}"))?;
            bytes.push(b'\n');
            Ok(bytes)
        })();
        let write_res = match bytes_res {
            Ok(bytes) => tokio::task::spawn_blocking(move || {
                write_checkpoint_atomic(&path, &bytes).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| format!("metering checkpoint write task failed: {e}"))
            .and_then(|r| r),
            Err(err) => Err(err),
        };
        if let Err(err) = &write_res {
            self.record_persistence_error(err);
        }
        write_res
    }

    fn is_authorized(&self, req: &Request<Incoming>) -> bool {
        let expected = self.config.gateway_credential.as_bytes();
        if expected.is_empty() {
            return false;
        }
        let auth_header = req.headers().get(hyper::header::AUTHORIZATION);
        let api_key_header = req.headers().get("x-api-key");
        if auth_header.is_none() && api_key_header.is_none() {
            return false;
        }
        if let Some(auth) = auth_header {
            let Some(token) = auth
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("Bearer "))
                .map(str::trim)
            else {
                return false;
            };
            if !constant_time_eq(token.as_bytes(), expected) {
                return false;
            }
        }
        if let Some(api_key) = api_key_header {
            let Some(key_str) = api_key.to_str().ok().map(str::trim) else {
                return false;
            };
            if !constant_time_eq(key_str.as_bytes(), expected) {
                return false;
            }
        }
        true
    }
}

/// Running local HTTP Eval Metering Gateway.
pub struct EvalMeteringGateway {
    addr: SocketAddr,
    gateway_credential: String,
    state: Arc<SharedGatewayState>,
    shutdown: CancellationToken,
    worker: Option<JoinHandle<()>>,
}

impl EvalMeteringGateway {
    pub async fn start(config: GatewayConfig) -> Result<Self, String> {
        if config.gateway_credential.trim().is_empty() {
            return Err("gateway_credential must not be empty".into());
        }
        if config.checkpoint_secret.trim().is_empty() {
            return Err("checkpoint_secret must not be empty".into());
        }
        let _ = identifier(&Value::String(config.profile.clone()), "gateway profile")
            .map_err(|e| e.to_string())?;

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| format!("failed to build gateway HTTP client: {e}"))?;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("failed to bind metering gateway on 127.0.0.1:0: {e}"))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("failed to read metering gateway local addr: {e}"))?;
        let gateway_credential = config.gateway_credential.clone();

        let shutdown = CancellationToken::new();
        let state = Arc::new(SharedGatewayState {
            config,
            client,
            accepting: AtomicBool::new(true),
            counters: Mutex::new(GatewayCounters::default()),
            checkpoint_lock: Mutex::new(()),
            checkpoint_error: std::sync::Mutex::new(None),
            in_flight_zero: Notify::new(),
            shutdown: shutdown.clone(),
        });
        state.persist_checkpoint().await?;

        let worker_shutdown = shutdown.clone();
        let worker_state = state.clone();
        let worker = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = worker_shutdown.cancelled() => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else {
                            break;
                        };
                        let conn_state = worker_state.clone();
                        tokio::spawn(async move {
                            let service = service_fn(move |req: Request<Incoming>| {
                                handle_gateway_request(conn_state.clone(), req)
                            });
                            let _ = http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service)
                                .await;
                        });
                    }
                }
            }
        });

        Ok(Self {
            addr,
            gateway_credential,
            state,
            shutdown,
            worker: Some(worker),
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    pub fn openai_v1_base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.addr.port())
    }

    /// Returns a loopback URL whose path matches `upstream_base_url` so provider route
    /// normalization in Maka sees the exact same path structure as the upstream endpoint.
    pub fn base_url_matching_upstream(&self, upstream_base_url: &str) -> String {
        let suffix = Url::parse(upstream_base_url)
            .ok()
            .map(|u| u.path().trim_end_matches('/').to_owned())
            .unwrap_or_else(|| "/v1".to_owned());
        format!("http://127.0.0.1:{}{}", self.addr.port(), suffix)
    }

    pub fn gateway_credential(&self) -> &str {
        &self.gateway_credential
    }

    pub fn persistence_error(&self) -> Option<String> {
        self.state.persistence_error()
    }

    /// Synchronously closes the admission window so any subsequent request receives HTTP 503
    /// without being forwarded or counted.
    pub fn stop_accepting(&self) {
        self.state.accepting.store(false, Ordering::SeqCst);
    }

    /// Closes admission, waits for all already-accepted in-flight requests to finish draining,
    /// marks the snapshot as `settled = true`, writes the final signed checkpoint, and returns
    /// the settled `ProviderMeteringCounts` — or `GatewayPersistenceError` (preserving the
    /// settled in-memory counts) if any checkpoint persistence failed.
    pub async fn report(&self) -> Result<ProviderMeteringCounts, GatewayPersistenceError> {
        self.stop_accepting();
        let deadline = tokio::time::Instant::now() + self.state.config.drain_timeout;
        let mut cancelled = false;
        loop {
            let notified = self.state.in_flight_zero.notified();
            {
                let mut counters = self.state.counters.lock().await;
                if counters.in_flight_requests == 0 {
                    counters.settled = true;
                    break;
                }
            }
            if cancelled {
                notified.await;
            } else if tokio::time::timeout_at(deadline, notified).await.is_err() {
                cancelled = true;
                self.state.shutdown.cancel();
            }
        }
        let persist_res = self.state.persist_checkpoint().await;
        let counts = self.state.counters.lock().await.snapshot();
        if let Err(message) = persist_res {
            return Err(GatewayPersistenceError {
                message,
                counts: Box::new(counts),
            });
        }
        if let Some(message) = self.persistence_error() {
            return Err(GatewayPersistenceError {
                message,
                counts: Box::new(counts),
            });
        }
        Ok(counts)
    }

    pub async fn close(mut self) -> Result<ProviderMeteringCounts, GatewayPersistenceError> {
        let result = self.report().await;
        self.shutdown.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.await;
        }
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamDisposition {
    Admitted,
    AuxiliaryNonBillable,
    ExplicitlyRefused,
    UnknownAdmission,
}

fn classify_http_disposition(
    method: &Method,
    req_path: &str,
    status_u16: u16,
    admitted: bool,
) -> UpstreamDisposition {
    let trimmed_path = req_path.trim_end_matches('/');
    match status_u16 {
        200..=299 if admitted => UpstreamDisposition::Admitted,
        200..=299
            if matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
                || trimmed_path.ends_with("/models")
                || trimmed_path.ends_with("/count_tokens") =>
        {
            UpstreamDisposition::AuxiliaryNonBillable
        }
        400..=499 if !matches!(status_u16, 408 | 499) => UpstreamDisposition::ExplicitlyRefused,
        _ => UpstreamDisposition::UnknownAdmission,
    }
}

impl Drop for EvalMeteringGateway {
    fn drop(&mut self) {
        self.state.accepting.store(false, Ordering::SeqCst);
        self.shutdown.cancel();
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

fn json_error_response(status: StatusCode, message: &str) -> Response<BoxBody> {
    let payload = serde_json::to_vec(&json!({ "error": message })).unwrap_or_default();
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(payload)).boxed())
        .unwrap()
}

async fn finish_in_flight(state: &SharedGatewayState) {
    let became_zero = {
        let mut counters = state.counters.lock().await;
        counters.in_flight_requests = counters.in_flight_requests.saturating_sub(1);
        counters.in_flight_requests == 0
    };
    let _ = state.persist_checkpoint().await;
    if became_zero {
        state.in_flight_zero.notify_waiters();
    }
}

async fn rollback_unstarted_request(state: &SharedGatewayState) {
    let became_zero = {
        let mut counters = state.counters.lock().await;
        counters.requests = counters.requests.saturating_sub(1);
        counters.in_flight_requests = counters.in_flight_requests.saturating_sub(1);
        counters.in_flight_requests == 0
    };
    if became_zero {
        state.in_flight_zero.notify_waiters();
    }
}

async fn handle_gateway_request(
    state: Arc<SharedGatewayState>,
    req: Request<Incoming>,
) -> Result<Response<BoxBody>, Infallible> {
    if !state.accepting.load(Ordering::SeqCst) {
        return Ok(json_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider proxy has settled",
        ));
    }
    if !state.is_authorized(&req) {
        return Ok(json_error_response(
            StatusCode::UNAUTHORIZED,
            "invalid eval gateway credential",
        ));
    }

    let method = req.method().clone();
    let uri_str = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    let req_path = req.uri().path().to_owned();
    let headers = req.headers().clone();

    let body_collect = Limited::new(req.into_body(), MAX_GATEWAY_REQUEST_BODY_BYTES).collect();
    let body_bytes = tokio::select! {
        _ = state.shutdown.cancelled() => {
            return Ok(json_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider proxy has settled",
            ));
        }
        collected = body_collect => match collected {
            Ok(collected) => collected.to_bytes(),
            Err(err) => {
                if err.is::<LengthLimitError>() {
                    return Ok(json_error_response(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "request body exceeds gateway limit",
                    ));
                }
                return Ok(json_error_response(
                    StatusCode::BAD_REQUEST,
                    "failed to read request body",
                ));
            }
        }
    };

    let observed = inspect_request_body(&body_bytes);

    let target_url = match join_upstream_url(&state.config.upstream_base_url, &uri_str) {
        Ok(u) => u,
        Err(_) => {
            return Ok(json_error_response(
                StatusCode::BAD_GATEWAY,
                "invalid upstream URL",
            ));
        }
    };
    let connect_url = match &state.config.transport_origin_override {
        Some(origin) => match rewrite_url_origin(&target_url, origin) {
            Ok(u) => u,
            Err(_) => {
                return Ok(json_error_response(
                    StatusCode::BAD_GATEWAY,
                    "invalid gateway transport origin override",
                ));
            }
        },
        None => target_url.clone(),
    };

    {
        let mut counters = state.counters.lock().await;
        if !state.accepting.load(Ordering::SeqCst) {
            return Ok(json_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider proxy has settled",
            ));
        }
        counters.requests += 1;
        counters.in_flight_requests += 1;
    }
    if state.persist_checkpoint().await.is_err() {
        rollback_unstarted_request(&state).await;
        return Ok(json_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "metering checkpoint persistence failed",
        ));
    }

    let reqwest_method =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::POST);
    let mut upstream_req = state.client.request(reqwest_method, &connect_url);
    if state.config.transport_origin_override.is_some() {
        upstream_req = upstream_req.header(UPSTREAM_TARGET_HEADER, &target_url);
    }
    let uses_x_api_key =
        headers.contains_key("x-api-key") && !headers.contains_key(hyper::header::AUTHORIZATION);
    for (name, value) in &headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "host"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "upgrade"
                | "accept-encoding"
                | "authorization"
                | "x-api-key"
                | UPSTREAM_TARGET_HEADER
        ) {
            continue;
        }
        if let Ok(val_str) = value.to_str() {
            upstream_req = upstream_req.header(name.as_str(), val_str);
        }
    }
    if !state.config.upstream_api_key.is_empty() {
        if uses_x_api_key {
            upstream_req = upstream_req.header("x-api-key", &state.config.upstream_api_key);
        } else {
            upstream_req = upstream_req.header(
                "authorization",
                format!("Bearer {}", state.config.upstream_api_key),
            );
        }
    }
    if !body_bytes.is_empty() {
        upstream_req = upstream_req.body(body_bytes);
    }

    let mut upstream_resp = tokio::select! {
        _ = state.shutdown.cancelled() => {
            {
                let mut counters = state.counters.lock().await;
                counters.unknown_requests += 1;
            }
            finish_in_flight(&state).await;
            return Ok(json_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider proxy drained before upstream response",
            ));
        }
        res = upstream_req.send() => match res {
            Ok(r) => r,
            Err(err) => {
                if !err.is_connect() {
                    let mut counters = state.counters.lock().await;
                    counters.unknown_requests += 1;
                }
                finish_in_flight(&state).await;
                return Ok(json_error_response(
                    StatusCode::BAD_GATEWAY,
                    "provider proxy failed",
                ));
            }
        }
    };

    let status_u16 = upstream_resp.status().as_u16();
    let is_ok = upstream_resp.status().is_success();
    let has_unhandled_content_encoding = upstream_resp
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|enc| {
            let trimmed = enc.trim();
            !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("identity")
        });
    let mut resp_builder =
        Response::builder().status(StatusCode::from_u16(status_u16).unwrap_or(StatusCode::OK));
    for (name, value) in upstream_resp.headers() {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "content-length" | "transfer-encoding" | "connection" | "keep-alive"
        ) {
            continue;
        }
        if let Ok(val_str) = value.to_str() {
            resp_builder = resp_builder.header(name.as_str(), val_str);
        }
    }

    let (tx, rx) = mpsc::channel::<Result<Frame<Bytes>, Infallible>>(16);
    let stream_state = state.clone();
    let stream_shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        let mut parser = UsageStreamParser::new();
        let mut counted = false;
        let mut downstream = Some(tx);
        let mut transport_complete = false;

        loop {
            let chunk = tokio::select! {
                _ = stream_shutdown.cancelled() => {
                    break;
                }
                next = upstream_resp.chunk() => next,
            };
            match chunk {
                Ok(Some(chunk)) => {
                    if !has_unhandled_content_encoding {
                        parser.push_bytes(&chunk);
                    }
                    if !counted && is_ok && parser.admitted() {
                        counted = true;
                        {
                            let mut counters = stream_state.counters.lock().await;
                            counters.admitted_requests += 1;
                        }
                        let _ = stream_state.persist_checkpoint().await;
                    }
                    // Downstream disconnect stops forwarding, never upstream metering drain.
                    if let Some(sender) = &downstream
                        && sender.send(Ok(Frame::data(chunk))).await.is_err()
                    {
                        downstream = None;
                    }
                }
                Ok(None) => {
                    transport_complete = true;
                    break;
                }
                Err(_) => {
                    break;
                }
            }
        }
        drop(downstream);

        let parsed = parser.finish_response(transport_complete);
        let admitted = counted || parsed.admitted;
        let effective_model = observed.model.or(parsed.model);
        match classify_http_disposition(&method, &req_path, status_u16, admitted) {
            UpstreamDisposition::Admitted => {
                {
                    let mut counters = stream_state.counters.lock().await;
                    if !counted {
                        counters.admitted_requests += 1;
                    }
                    if let Some(model) = &effective_model {
                        counters.models.insert(model.clone());
                    }
                    for name in &observed.tool_names {
                        counters.tool_names.insert(name.clone());
                    }
                    if let Some(u) = parsed.usage {
                        counters.has_usage = true;
                        counters.total_usage.accumulate_exact(&u);
                        if let Some(model) = &effective_model {
                            counters
                                .model_usage
                                .entry(model.clone())
                                .or_default()
                                .accumulate_exact(&u);
                        }
                        if parsed.settled {
                            counters.usage_requests += 1;
                        }
                    }
                }
                if !counted {
                    let _ = stream_state.persist_checkpoint().await;
                }
            }
            UpstreamDisposition::UnknownAdmission => {
                let mut counters = stream_state.counters.lock().await;
                counters.unknown_requests += 1;
            }
            UpstreamDisposition::AuxiliaryNonBillable | UpstreamDisposition::ExplicitlyRefused => {}
        }
        finish_in_flight(&stream_state).await;
    });

    let stream = tokio_stream_from_receiver(rx);
    let body = http_body_util::BodyExt::boxed(StreamBody::new(stream));
    Ok(resp_builder.body(body).unwrap())
}

fn tokio_stream_from_receiver<T>(
    mut rx: mpsc::Receiver<T>,
) -> impl futures_util::Stream<Item = T> + Send + Sync + 'static
where
    T: Send + 'static,
{
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

fn write_checkpoint_atomic(final_path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let file_name = final_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "atomic write path has no file name",
            )
        })?;
    let parent_dir = match final_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => std::path::Path::new("."),
    };
    if parent_dir != std::path::Path::new(".") {
        std::fs::create_dir_all(parent_dir)?;
    }
    let temp_path = parent_dir.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_file = options.open(&temp_path)?;
    let res = (|| -> std::io::Result<()> {
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);
        std::fs::rename(&temp_path, final_path)?;
        if let Ok(dir) = std::fs::File::open(parent_dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measurement::{
        MeteringState, derive_metering, recover_external_metering_checkpoint,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TEST_SECRET: &str = "0123456789abcdef0123456789abcdef";
    const TEST_TOKEN: &str = "attempt-gateway-token";

    fn loopback_connect_permitted() -> bool {
        let Ok(probe) = std::net::TcpListener::bind("127.0.0.1:0") else {
            return false;
        };
        let Ok(addr) = probe.local_addr() else {
            return false;
        };
        !std::net::TcpStream::connect(addr)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
    }

    fn test_gateway_config(
        upstream_base_url: String,
        checkpoint_path: Option<PathBuf>,
    ) -> GatewayConfig {
        GatewayConfig {
            upstream_base_url,
            transport_origin_override: None,
            upstream_api_key: "real-upstream-secret".into(),
            profile: "maka".into(),
            gateway_credential: TEST_TOKEN.into(),
            checkpoint_path,
            checkpoint_secret: TEST_SECRET.into(),
            drain_timeout: DEFAULT_GATEWAY_DRAIN_TIMEOUT,
        }
    }

    enum MockReply {
        DropBeforeHeaders,
        Json(&'static str, &'static str),
        TruncatedSse(&'static str),
    }

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        let mut expected_total = None;
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
            if expected_total.is_none()
                && let Some(header_end) = all.windows(4).position(|w| w == b"\r\n\r\n")
            {
                let headers = String::from_utf8_lossy(&all[..header_end]);
                let content_len = headers
                    .lines()
                    .find_map(|line| {
                        let (name, val) = line.split_once(':')?;
                        name.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then(|| val.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                expected_total = Some(header_end + 4 + content_len);
            }
            if expected_total.is_some_and(|total| all.len() >= total) {
                break;
            }
        }
        all
    }

    async fn spawn_mock_upstream(replies: Vec<MockReply>) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let _ = read_http_request(&mut socket).await;
                match reply {
                    MockReply::DropBeforeHeaders => drop(socket),
                    MockReply::Json(status, body) => {
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    MockReply::TruncatedSse(body) => {
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                }
            }
        });
        (addr, handle)
    }

    async fn post_empty_json(
        client: &reqwest::Client,
        gateway: &EvalMeteringGateway,
    ) -> reqwest::Response {
        client
            .post(format!("{}/chat/completions", gateway.openai_v1_base_url()))
            .bearer_auth(TEST_TOKEN)
            .body("{}")
            .send()
            .await
            .unwrap()
    }

    #[test]
    fn usage_and_stream_parsers_validate_counters_and_utf8_chunk_boundaries() {
        let explicit_zero_total = json!({
            "object": "chat.completion",
            "usage": {"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 0}
        });
        let parsed = usage_from_event(explicit_zero_total.as_object().unwrap()).unwrap();
        assert_eq!(parsed.input_tokens, 100);
        assert_eq!(parsed.output_tokens, 20);
        assert_eq!(parsed.total_tokens, 0);

        let omitted_total = json!({
            "object": "chat.completion",
            "usage": {"prompt_tokens": 100, "completion_tokens": 20}
        });
        assert_eq!(
            usage_from_event(omitted_total.as_object().unwrap())
                .unwrap()
                .total_tokens,
            120
        );

        let explicit_zero_all = json!({
            "object": "chat.completion",
            "usage": {"prompt_tokens": 0, "completion_tokens": 0}
        });
        let zero_all = usage_from_event(explicit_zero_all.as_object().unwrap()).unwrap();
        assert_eq!(
            (
                zero_all.input_tokens,
                zero_all.output_tokens,
                zero_all.total_tokens
            ),
            (0, 0, 0)
        );

        for incomplete in [
            json!({"object": "chat.completion", "usage": {}}),
            json!({"object": "chat.completion", "usage": {"prompt_tokens": 100}}),
            json!({"object": "chat.completion", "usage": {"prompt_tokens": "bad", "completion_tokens": 1}}),
            json!({"object": "chat.completion", "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": -1}}),
        ] {
            assert!(
                usage_from_event(incomplete.as_object().unwrap()).is_none(),
                "incomplete or malformed usage must not synthesize counters: {incomplete}"
            );
        }

        let mut parser = UsageStreamParser::new();
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":15,\"output_tokens\":0,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":2}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你好世界\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        // Split every 5 bytes so multi-byte UTF-8 characters are broken across chunks.
        for chunk in sse.as_bytes().chunks(5) {
            parser.push_bytes(chunk);
        }
        let parsed = parser.finish_response(true);
        assert!(parsed.admitted);
        assert!(parsed.settled);
        assert_eq!(
            parsed.usage,
            Some(NormalizedUsage {
                input_tokens: 21,
                output_tokens: 3,
                cache_read_tokens: 4,
                cache_write_tokens: 2,
                reasoning_tokens: 0,
                total_tokens: 24,
            })
        );

        assert_eq!(
            classify_http_disposition(&Method::POST, "/v1/chat/completions", 200, true),
            UpstreamDisposition::Admitted
        );
        assert_eq!(
            classify_http_disposition(&Method::POST, "/v1/chat/completions", 200, false),
            UpstreamDisposition::UnknownAdmission
        );
        assert_eq!(
            classify_http_disposition(&Method::GET, "/v1/models", 200, false),
            UpstreamDisposition::AuxiliaryNonBillable
        );
        assert_eq!(
            classify_http_disposition(&Method::POST, "/v1/messages/count_tokens", 200, false),
            UpstreamDisposition::AuxiliaryNonBillable
        );
        for status in [400, 429] {
            assert_eq!(
                classify_http_disposition(&Method::POST, "/v1/chat/completions", status, false),
                UpstreamDisposition::ExplicitlyRefused
            );
        }
        for status in [408, 499, 502] {
            assert_eq!(
                classify_http_disposition(&Method::POST, "/v1/chat/completions", status, false),
                UpstreamDisposition::UnknownAdmission
            );
        }

        // OpenAI /v1/responses response.incomplete settles usage without `[DONE]`.
        let mut resp_parser = UsageStreamParser::new();
        resp_parser.push(
            "event: response.created\n\
             data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"deepseek-v4-flash\"}}\n\n\
             event: response.incomplete\n\
             data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_1\",\"model\":\"deepseek-v4-flash\",\"usage\":{\"input_tokens\":40,\"output_tokens\":12,\"total_tokens\":52}}}\n\n",
        );
        let resp_parsed = resp_parser.finish_response(true);
        assert!(resp_parsed.admitted);
        assert!(resp_parsed.settled);
        assert_eq!(resp_parsed.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(
            resp_parsed.usage,
            Some(NormalizedUsage {
                input_tokens: 40,
                output_tokens: 12,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 52,
            })
        );

        // Anthropic message_delta with input_tokens=0 and explicit total_tokens=0 preserves message_start input_tokens.
        let mut ant_parser = UsageStreamParser::new();
        ant_parser.push(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"model\":\"claude-sonnet\",\"usage\":{\"input_tokens\":100,\"output_tokens\":1}}}\n\n\
             event: message_delta\n\
             data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":0,\"output_tokens\":9,\"total_tokens\":0}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
        );
        let ant_parsed = ant_parser.finish_response(true);
        assert!(ant_parsed.settled);
        assert_eq!(ant_parsed.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(
            ant_parsed.usage,
            Some(NormalizedUsage {
                input_tokens: 100,
                output_tokens: 9,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 0,
            })
        );
    }

    #[tokio::test]
    async fn gateway_authenticates_forwards_untouched_payload_enforces_cutoff_and_signs_checkpoint()
    {
        if !loopback_connect_permitted() {
            return;
        }
        let expected_raw_body = br#"{"model":"deepseek-chat","tools":[{"type":"function","function":{"name":"WebSearch"}},{"name":"web_fetch","type":"custom"},{"type":"web_search_20250305"}]}"#.to_vec();
        let expected_for_server = expected_raw_body.clone();

        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = upstream_listener.accept().await else {
                    break;
                };
                let expected_bytes = expected_for_server.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |req: Request<Incoming>| {
                        let expected_bytes = expected_bytes.clone();
                        async move {
                            assert_eq!(req.uri().path(), "/v1/chat/completions");
                            assert_eq!(
                                req.headers()
                                    .get("authorization")
                                    .and_then(|v| v.to_str().ok()),
                                Some("Bearer real-upstream-secret")
                            );
                            let body = req.into_body().collect().await.unwrap().to_bytes();
                            assert_eq!(body.as_ref(), expected_bytes.as_slice());

                            let sse = concat!(
                                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\n",
                                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":25,\"prompt_cache_hit_tokens\":60,\"completion_tokens_details\":{\"reasoning_tokens\":10},\"total_tokens\":125}}\n\n",
                                "data: [DONE]\n\n"
                            );
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header("content-type", "text/event-stream")
                                    .body(Full::new(Bytes::from(sse)))
                                    .unwrap(),
                            )
                        }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });

        let tmp = tempfile::tempdir().unwrap();
        let agent_dir = tmp.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let gateway = EvalMeteringGateway::start(test_gateway_config(
            format!("http://{upstream_addr}/v1"),
            Some(agent_dir.join("maka.provider-usage.json")),
        ))
        .await
        .unwrap();

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // 1. Unauthorized request is rejected with 401 and not counted.
        let unauth = client
            .post(format!("{}/chat/completions", gateway.openai_v1_base_url()))
            .bearer_auth("wrong-token")
            .header("content-type", "application/json")
            .body(br#"{"model":"deepseek-chat"}"#.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), reqwest::StatusCode::UNAUTHORIZED);

        // 2. Authorized request forwards exact payload and records SSE usage.
        let resp = client
            .post(format!("{}/chat/completions", gateway.openai_v1_base_url()))
            .bearer_auth(TEST_TOKEN)
            .header("content-type", "application/json")
            .body(expected_raw_body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert!(resp.text().await.unwrap().contains("[DONE]"));

        // 3. Stop accepting and verify subsequent requests get 503 without inflating counts.
        gateway.stop_accepting();
        let refused = post_empty_json(&client, &gateway).await;
        assert_eq!(refused.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);

        let counts = gateway.close().await.unwrap();
        upstream_task.abort();

        assert!(counts.settled);
        assert_eq!(counts.requests, 1);
        assert_eq!(counts.admitted_requests, 1);
        assert_eq!(counts.usage_requests, 1);
        assert_eq!(counts.unknown_requests, 0);
        assert_eq!(counts.models, vec!["deepseek-chat".to_owned()]);
        assert_eq!(
            counts.tool_names,
            vec![
                "WebSearch".to_owned(),
                "web_fetch".to_owned(),
                "web_search_20250305".to_owned(),
            ]
        );
        assert_eq!(
            counts.usage,
            Some(NormalizedUsage {
                input_tokens: 100,
                output_tokens: 25,
                cache_read_tokens: 60,
                cache_write_tokens: 0,
                reasoning_tokens: 10,
                total_tokens: 125,
            })
        );

        let recovered = recover_external_metering_checkpoint(tmp.path(), "maka", TEST_SECRET)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.counts, counts);
        assert_eq!(recovered.metering_state, MeteringState::Complete);
        assert_eq!(recovered.cost_usd, None);
        assert!(recover_external_metering_checkpoint(tmp.path(), "maka", "wrong-secret").is_err());
    }

    #[tokio::test]
    async fn disconnected_client_still_drains_upstream_to_complete_usage() {
        use std::time::Duration;
        use tokio::sync::oneshot;

        if !loopback_connect_permitted() {
            return;
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap();
        let (continue_tx, continue_rx) = oneshot::channel();
        let (finish_tx, finish_rx) = oneshot::channel();
        let provider = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
                )
                .await
                .unwrap();
            let first = "data: {\"object\":\"chat.completion.chunk\",\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":1,\"total_tokens\":101}}\n\n";
            socket
                .write_all(format!("{:x}\r\n{first}\r\n", first.len()).as_bytes())
                .await
                .unwrap();
            continue_rx.await.unwrap();
            for _ in 0..40 {
                let text = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[]}\n\n";
                if socket
                    .write_all(format!("{:x}\r\n{text}\r\n", text.len()).as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
            finish_rx.await.unwrap();
            let last = "data: {\"object\":\"chat.completion.chunk\",\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"total_tokens\":150}}\n\ndata: [DONE]\n\n";
            let _ = socket
                .write_all(format!("{:x}\r\n{last}\r\n0\r\n\r\n", last.len()).as_bytes())
                .await;
        });

        let gateway = EvalMeteringGateway::start(test_gateway_config(
            format!("http://{upstream_addr}/v1"),
            None,
        ))
        .await
        .unwrap();

        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", gateway.port()))
            .await
            .unwrap();
        client
            .write_all(
                format!(
                    "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TEST_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut received = Vec::new();
        let mut buf = [0; 1024];
        while !String::from_utf8_lossy(&received).contains("101}") {
            let n = client.read(&mut buf).await.unwrap();
            assert!(n > 0);
            received.extend_from_slice(&buf[..n]);
        }
        drop(client);
        continue_tx.send(()).unwrap();
        let premature = tokio::time::timeout(Duration::from_millis(150), gateway.report()).await;
        assert!(
            premature.is_err(),
            "report must wait for the upstream stream to finish draining"
        );
        finish_tx.send(()).unwrap();
        let counts = gateway.close().await.unwrap();
        provider.await.unwrap();
        assert_eq!(
            derive_metering(&counts).metering_state,
            MeteringState::Complete
        );
        assert_eq!(counts.usage.unwrap().total_tokens, 150);
    }

    #[tokio::test]
    async fn incomplete_or_ambiguous_upstream_responses_never_produce_complete_bills() {
        if !loopback_connect_permitted() {
            return;
        }
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // 1. Admitted 200 with empty `{}` usage => Unknown, cost_usd == None.
        let (addr, server) = spawn_mock_upstream(vec![MockReply::Json(
            "200 OK",
            r#"{"object":"chat.completion","usage":{}}"#,
        )])
        .await;
        let gw = EvalMeteringGateway::start(test_gateway_config(format!("http://{addr}/v1"), None))
            .await
            .unwrap();
        let resp = post_empty_json(&client, &gw).await;
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let _ = resp.bytes().await.unwrap();
        let counts = gw.close().await.unwrap();
        server.await.unwrap();
        let derived = derive_metering(&counts);
        assert_eq!(counts.admitted_requests, 1);
        assert_eq!(counts.usage_requests, 0);
        assert!(counts.usage.is_none());
        assert_eq!(derived.metering_state, MeteringState::Unknown);
        assert_eq!(derived.cost_usd, None);

        // 2. Truncated SSE stream without `[DONE]` => LowerBound, cost_usd == None.
        let (addr, server) = spawn_mock_upstream(vec![MockReply::TruncatedSse(
            "data: {\"object\":\"chat.completion.chunk\",\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":1,\"total_tokens\":101}}\n\n",
        )])
        .await;
        let gw = EvalMeteringGateway::start(test_gateway_config(format!("http://{addr}/v1"), None))
            .await
            .unwrap();
        let _ = post_empty_json(&client, &gw).await.bytes().await.unwrap();
        let counts = gw.close().await.unwrap();
        server.await.unwrap();
        let derived = derive_metering(&counts);
        assert_eq!(counts.admitted_requests, 1);
        assert_eq!(counts.usage_requests, 0);
        assert_eq!(counts.usage.unwrap().total_tokens, 101);
        assert_eq!(derived.metering_state, MeteringState::LowerBound);
        assert_eq!(derived.cost_usd, None);

        // 3. Explicit 429 refusal + 200 OK => Complete with cost_usd populated for deepseek-v4-flash.
        let (addr, server) = spawn_mock_upstream(vec![
            MockReply::Json("429 Too Many Requests", r#"{"error":"rate_limited"}"#),
            MockReply::Json(
                "200 OK",
                r#"{"object":"chat.completion","model":"deepseek-v4-flash","usage":{"prompt_tokens":50,"completion_tokens":10,"total_tokens":60}}"#,
            ),
        ])
        .await;
        let gw = EvalMeteringGateway::start(test_gateway_config(format!("http://{addr}/v1"), None))
            .await
            .unwrap();
        for _ in 0..2 {
            let _ = post_empty_json(&client, &gw).await.bytes().await.unwrap();
        }
        let counts = gw.close().await.unwrap();
        server.await.unwrap();
        let derived = derive_metering(&counts);
        assert_eq!(counts.requests, 2);
        assert_eq!(counts.admitted_requests, 1);
        assert_eq!(counts.usage_requests, 1);
        assert_eq!(counts.unknown_requests, 0);
        assert_eq!(derived.metering_state, MeteringState::Complete);
        assert!(derived.cost_usd.is_some());

        // 4. 200 OK + pre-header socket drop + 502 Bad Gateway => both ambiguous failures mark
        // `unknown_requests` and keep metering at LowerBound with cost_usd == None.
        let (addr, server) = spawn_mock_upstream(vec![
            MockReply::Json(
                "200 OK",
                r#"{"object":"chat.completion","usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}"#,
            ),
            MockReply::DropBeforeHeaders,
            MockReply::Json("502 Bad Gateway", r#"{"error":"bad_gateway"}"#),
        ])
        .await;
        let gw = EvalMeteringGateway::start(test_gateway_config(format!("http://{addr}/v1"), None))
            .await
            .unwrap();
        let mut statuses = Vec::new();
        for _ in 0..3 {
            let resp = post_empty_json(&client, &gw).await;
            statuses.push(resp.status().as_u16());
            let _ = resp.bytes().await.unwrap();
        }
        assert_eq!(statuses, vec![200, 502, 502]);
        let counts = gw.close().await.unwrap();
        server.await.unwrap();
        let derived = derive_metering(&counts);
        assert_eq!(counts.requests, 3);
        assert_eq!(counts.admitted_requests, 1);
        assert_eq!(counts.usage_requests, 1);
        assert_eq!(counts.unknown_requests, 2);
        assert_eq!(derived.metering_state, MeteringState::LowerBound);
        assert_eq!(derived.cost_usd, None);
    }

    #[tokio::test]
    async fn checkpoint_persistence_failure_fails_startup_runtime_and_close() {
        let tmp = tempfile::tempdir().unwrap();
        let blocked_path = tmp.path().join("blocked-checkpoint.json");
        std::fs::create_dir(&blocked_path).unwrap();

        let startup = EvalMeteringGateway::start(test_gateway_config(
            "http://127.0.0.1:1/v1".into(),
            Some(blocked_path),
        ))
        .await;
        assert!(
            startup.is_err(),
            "a configured checkpoint that cannot be persisted must fail startup"
        );

        let close_cp = tmp.path().join("final-checkpoint.json");
        let close_gateway = EvalMeteringGateway::start(test_gateway_config(
            "http://127.0.0.1:1/v1".into(),
            Some(close_cp.clone()),
        ))
        .await
        .unwrap();
        std::fs::remove_file(&close_cp).unwrap();
        std::fs::create_dir(&close_cp).unwrap();
        let close_err = close_gateway
            .close()
            .await
            .expect_err("normal close() must expose final checkpoint persistence failure");
        assert!(close_err.counts.settled);
        assert_eq!(close_err.counts.requests, 0);

        if !loopback_connect_permitted() {
            return;
        }
        let runtime_cp = tmp.path().join("runtime-checkpoint.json");
        let gateway = EvalMeteringGateway::start(test_gateway_config(
            "http://127.0.0.1:1/v1".into(),
            Some(runtime_cp.clone()),
        ))
        .await
        .unwrap();
        std::fs::remove_file(&runtime_cp).unwrap();
        std::fs::create_dir(&runtime_cp).unwrap();

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let resp = post_empty_json(&client, &gateway).await;
        assert_eq!(resp.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert!(gateway.persistence_error().is_some());

        // Even if the path becomes writable again before close, the sticky runtime persistence
        // error must still be surfaced.
        std::fs::remove_dir(&runtime_cp).unwrap();
        let err = gateway
            .close()
            .await
            .expect_err("sticky runtime persistence failure must be surfaced by close");
        assert_eq!(err.counts.requests, 0);
        assert_eq!(err.counts.admitted_requests, 0);
    }

    #[tokio::test]
    async fn gateway_strips_zstd_ignores_auxiliary_endpoints_and_bounds_drain() {
        if !loopback_connect_permitted() {
            return;
        }

        // 1. Gateway strips client Accept-Encoding (e.g. zstd) and does not count GET /models
        // or POST /messages/count_tokens as unknown_requests.
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            for idx in 0..3 {
                let (mut socket, _) = upstream_listener.accept().await.unwrap();
                let req_bytes = read_http_request(&mut socket).await;
                let req_text = String::from_utf8_lossy(&req_bytes);
                assert!(
                    !req_text.to_ascii_lowercase().contains("zstd"),
                    "gateway must strip client zstd Accept-Encoding: {req_text}"
                );
                let body = match idx {
                    0 => r#"{"object":"list","data":[{"id":"deepseek-v4-flash"}]}"#,
                    1 => r#"{"input_tokens":12}"#,
                    _ => {
                        r#"{"object":"chat.completion","model":"deepseek-v4-flash","usage":{"prompt_tokens":30,"completion_tokens":5,"total_tokens":35}}"#
                    }
                };
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });

        let gw = EvalMeteringGateway::start(test_gateway_config(
            format!("http://{upstream_addr}/v1"),
            None,
        ))
        .await
        .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let r0 = client
            .get(format!("{}/models", gw.openai_v1_base_url()))
            .bearer_auth(TEST_TOKEN)
            .header("accept-encoding", "gzip, deflate, br, zstd")
            .send()
            .await
            .unwrap();
        assert_eq!(r0.status(), reqwest::StatusCode::OK);
        let _ = r0.bytes().await.unwrap();

        let r1 = client
            .post(format!("{}/messages/count_tokens", gw.openai_v1_base_url()))
            .bearer_auth(TEST_TOKEN)
            .header("accept-encoding", "zstd")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(r1.status(), reqwest::StatusCode::OK);
        let _ = r1.bytes().await.unwrap();

        let r2 = post_empty_json(&client, &gw).await;
        assert_eq!(r2.status(), reqwest::StatusCode::OK);
        let _ = r2.bytes().await.unwrap();

        let counts = gw.close().await.unwrap();
        upstream_task.await.unwrap();
        assert_eq!(counts.requests, 3);
        assert_eq!(counts.admitted_requests, 1);
        assert_eq!(counts.usage_requests, 1);
        assert_eq!(counts.unknown_requests, 0);
        assert_eq!(
            derive_metering(&counts).metering_state,
            MeteringState::Complete
        );

        // 2. Hanging upstream SSE stream does not deadlock gateway.close() past drain_timeout.
        let hang_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hang_addr = hang_listener.local_addr().unwrap();
        let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let hang_task = tokio::spawn(async move {
            let (mut socket, _) = hang_listener.accept().await.unwrap();
            let _ = read_http_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
                )
                .await
                .unwrap();
            let first = "data: {\"object\":\"chat.completion.chunk\",\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":1,\"total_tokens\":11}}\n\n";
            socket
                .write_all(format!("{:x}\r\n{first}\r\n", first.len()).as_bytes())
                .await
                .unwrap();
            let _ = release_rx.await;
        });

        let mut fast_cfg = test_gateway_config(format!("http://{hang_addr}/v1"), None);
        fast_cfg.drain_timeout = Duration::from_millis(80);
        let hang_gw = EvalMeteringGateway::start(fast_cfg).await.unwrap();
        let _resp = post_empty_json(&client, &hang_gw).await;
        let close_res = tokio::time::timeout(Duration::from_secs(2), hang_gw.close())
            .await
            .expect("close must be bounded by drain_timeout")
            .unwrap();
        hang_task.abort();
        assert!(close_res.settled);
        assert_eq!(close_res.in_flight_requests, 0);
        assert_eq!(close_res.admitted_requests, 1);
        assert_eq!(close_res.usage_requests, 0);
        assert_eq!(
            derive_metering(&close_res).metering_state,
            MeteringState::LowerBound
        );
    }
}
