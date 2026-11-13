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

//! Single-writer append-only experiment journal, atomic attempt settlement,
//! critical artifact verification, and deterministic state reconstruction.

use crate::contract::{
    AttemptCancellation, AttemptMetering, AttemptPhase, CleanupState, ContractError,
    CriticalArtifactsState, EgressAuditState, EvalResultStatus, ExecutionOutcome, ExperimentSpec,
    SchemaVersion, V1CellAttempt, V2AttemptRecord, VerifierOutcome, canonical_json_string,
    decode_v1_cell_attempt, parse_experiment_spec, sha256_hex,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

pub const MAKA_RUNTIME_ARTIFACT_PATH: &str = "/logs/artifacts/maka-runtime-host";
pub const MAKA_SUBJECT_STDOUT_PATH: &str = "/logs/artifacts/maka-subject.stdout.txt";
pub const MAKA_SUBJECT_STDERR_PATH: &str = "/logs/artifacts/maka-subject.stderr.txt";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Contract error: {0}")]
    Contract(#[from] ContractError),
    #[error("{0}: experiment already has an active writer")]
    ActiveWriter(String),
    #[error("{0}")]
    Integrity(String),
}

/// Exclusive single-writer lock guard held for the duration of a store write session.
#[derive(Debug)]
pub struct WriterGuard {
    _file: File,
}

impl WriterGuard {
    pub fn acquire(dir: &Path) -> Result<Self, StoreError> {
        fs::create_dir_all(dir)?;
        let lock_path = dir.join(".writer.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&lock_path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                Err(StoreError::ActiveWriter(lock_path.display().to_string()))
            }
            Err(std::fs::TryLockError::Error(err)) => Err(StoreError::Io(err)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEntry {
    LaunchIntent {
        cell_id: String,
        sequence: u32,
        started_at: u64,
        intent_id: String,
    },
    ResourceTagged {
        cell_id: String,
        sequence: u32,
        resource_kind: String,
        resource_id: String,
        operation_id: Option<String>,
        invocation_id: Option<String>,
    },
    Checkpoint {
        cell_id: String,
        sequence: u32,
        recorded_at: u64,
        metering: AttemptMetering,
    },
    Settled {
        attempt: Box<V2AttemptRecord>,
    },
    PostHocRecovery {
        cell_id: String,
        sequence: u32,
        recovered_at: u64,
        metering: Option<AttemptMetering>,
        cleanup_confirmed: bool,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnsettledAttempt {
    pub cell_id: String,
    pub sequence: u32,
    pub started_at: u64,
    pub intent_id: String,
    pub resource_kind: Option<String>,
    pub resource_id: Option<String>,
    pub operation_id: Option<String>,
    pub invocation_id: Option<String>,
    pub latest_metering: AttemptMetering,
}

#[derive(Debug, Clone, Default)]
pub struct ReconstructedStoreState {
    pub settled_v2: BTreeMap<String, Vec<V2AttemptRecord>>,
    pub settled_v1: BTreeMap<String, Vec<V1CellAttempt>>,
    pub unsettled: BTreeMap<(String, u32), UnsettledAttempt>,
}

impl ReconstructedStoreState {
    fn settled_count_for(&self, cell_id: &str) -> u32 {
        self.settled_v2.get(cell_id).map_or(0, |v| v.len() as u32)
    }

    fn has_unsettled_for(&self, cell_id: &str) -> bool {
        self.unsettled.keys().any(|(c, _)| c == cell_id)
    }

    pub fn next_sequence(&self, cell_id: &str) -> u32 {
        let settled_next = self.settled_count_for(cell_id).saturating_add(1);
        let unsettled_next = self
            .unsettled
            .keys()
            .filter(|(c, _)| c == cell_id)
            .map(|(_, s)| s.saturating_add(1))
            .max()
            .unwrap_or(1);
        settled_next.max(unsettled_next)
    }

    fn find_settled_mut(
        &mut self,
        cell_id: &str,
        sequence: u32,
    ) -> Option<(&mut V2AttemptRecord, &mut V1CellAttempt)> {
        if sequence == 0 {
            return None;
        }
        let idx = (sequence - 1) as usize;
        let v2 = self.settled_v2.get_mut(cell_id)?.get_mut(idx)?;
        let v1 = self.settled_v1.get_mut(cell_id)?.get_mut(idx)?;
        Some((v2, v1))
    }

    pub fn apply_entry(&mut self, entry: JournalEntry) -> Result<(), StoreError> {
        match entry {
            JournalEntry::LaunchIntent {
                cell_id,
                sequence,
                started_at,
                intent_id,
            } => {
                if cell_id.is_empty() || intent_id.is_empty() {
                    return Err(StoreError::Integrity(
                        "launch intent requires non-empty cell_id and intent_id".into(),
                    ));
                }
                if self.has_unsettled_for(&cell_id) {
                    return Err(StoreError::Integrity(format!(
                        "cell {cell_id} already has an unsettled attempt"
                    )));
                }
                let expected = self.settled_count_for(&cell_id).saturating_add(1);
                if sequence != expected {
                    return Err(StoreError::Integrity(format!(
                        "attempt sequence must be {expected}"
                    )));
                }
                self.unsettled.insert(
                    (cell_id.clone(), sequence),
                    UnsettledAttempt {
                        cell_id,
                        sequence,
                        started_at,
                        intent_id,
                        resource_kind: None,
                        resource_id: None,
                        operation_id: None,
                        invocation_id: None,
                        latest_metering: AttemptMetering::Unknown,
                    },
                );
            }
            JournalEntry::ResourceTagged {
                cell_id,
                sequence,
                resource_kind,
                resource_id,
                operation_id,
                invocation_id,
            } => {
                if resource_kind.is_empty() || resource_id.is_empty() {
                    return Err(StoreError::Integrity(
                        "resource tag requires non-empty resource_kind and resource_id".into(),
                    ));
                }
                let Some(unsettled) = self.unsettled.get_mut(&(cell_id.clone(), sequence)) else {
                    return Err(StoreError::Integrity(format!(
                        "resource tag references unknown unsettled attempt {cell_id}#{sequence}"
                    )));
                };
                unsettled.resource_kind = Some(resource_kind);
                unsettled.resource_id = Some(resource_id);
                unsettled.operation_id = operation_id;
                unsettled.invocation_id = invocation_id;
            }
            JournalEntry::Checkpoint {
                cell_id,
                sequence,
                metering,
                ..
            } => {
                metering.validate()?;
                let Some(unsettled) = self.unsettled.get_mut(&(cell_id.clone(), sequence)) else {
                    return Err(StoreError::Integrity(format!(
                        "checkpoint references unknown unsettled attempt {cell_id}#{sequence}"
                    )));
                };
                if !matches!(metering, AttemptMetering::Unknown)
                    || matches!(unsettled.latest_metering, AttemptMetering::Unknown)
                {
                    unsettled.latest_metering = metering;
                }
            }
            JournalEntry::Settled { attempt } => {
                attempt.validate()?;
                let v1 = attempt.project_v1();
                let _ = decode_v1_cell_attempt(&serde_json::to_value(&v1)?)?;
                let key = (attempt.cell_id.clone(), attempt.sequence);
                if self.unsettled.contains_key(&key) {
                    self.unsettled.remove(&key);
                } else {
                    if self.has_unsettled_for(&attempt.cell_id) {
                        return Err(StoreError::Integrity(format!(
                            "cell {} has an unsettled attempt blocking out-of-order settlement #{}",
                            attempt.cell_id, attempt.sequence
                        )));
                    }
                    let expected = self.settled_count_for(&attempt.cell_id).saturating_add(1);
                    if attempt.sequence != expected {
                        return Err(StoreError::Integrity(format!(
                            "attempt sequence must be {expected}"
                        )));
                    }
                }
                self.settled_v2
                    .entry(key.0.clone())
                    .or_default()
                    .push(*attempt);
                self.settled_v1.entry(key.0).or_default().push(v1);
            }
            JournalEntry::PostHocRecovery {
                cell_id,
                sequence,
                recovered_at,
                metering,
                cleanup_confirmed,
                reason,
            } => {
                if let Some(ref m) = metering {
                    m.validate()?;
                }
                let key = (cell_id.clone(), sequence);
                if let Some(unsettled) = self.unsettled.remove(&key) {
                    let effective_metering = match metering {
                        Some(m) if !matches!(m, AttemptMetering::Unknown) => m,
                        _ => unsettled.latest_metering,
                    };
                    let record = V2AttemptRecord {
                        schema_version: SchemaVersion::V2,
                        cell_id: cell_id.clone(),
                        sequence,
                        started_at: unsettled.started_at,
                        completed_at: recovered_at,
                        duration_ms: recovered_at.saturating_sub(unsettled.started_at),
                        phase: AttemptPhase::Recovered,
                        execution_outcome: if cleanup_confirmed {
                            ExecutionOutcome::InfraFailed
                        } else {
                            ExecutionOutcome::Indeterminate
                        },
                        verifier_outcome: VerifierOutcome::Skipped,
                        metering: effective_metering,
                        cleanup_state: if cleanup_confirmed {
                            CleanupState::Confirmed
                        } else {
                            CleanupState::Unconfirmed {
                                reason: reason
                                    .clone()
                                    .unwrap_or_else(|| "executor cleanup did not settle".into()),
                            }
                        },
                        audit_state: EgressAuditState::NotRequired,
                        critical_artifacts_state: CriticalArtifactsState::Present,
                        cancellation: AttemptCancellation::None,
                        recovered_post_hoc: true,
                        failure_reason: Some(
                            reason.unwrap_or_else(|| "recovered after interrupted attempt".into()),
                        ),
                        artifacts: Vec::new(),
                    };
                    let v1 = record.project_v1();
                    self.settled_v2
                        .entry(cell_id.clone())
                        .or_default()
                        .push(record);
                    self.settled_v1.entry(cell_id).or_default().push(v1);
                } else if let Some((existing, existing_v1)) =
                    self.find_settled_mut(&cell_id, sequence)
                {
                    existing.recovered_post_hoc = true;
                    existing.phase = AttemptPhase::Recovered;
                    if let Some(m) = metering
                        && !matches!(m, AttemptMetering::Unknown)
                    {
                        existing.metering = m;
                    }
                    if cleanup_confirmed
                        && matches!(existing.cleanup_state, CleanupState::Unconfirmed { .. })
                    {
                        existing.cleanup_state = CleanupState::Confirmed;
                        if existing.execution_outcome == ExecutionOutcome::Indeterminate
                            && existing.verifier_outcome == VerifierOutcome::Skipped
                            && existing.cancellation == AttemptCancellation::None
                        {
                            existing.execution_outcome = ExecutionOutcome::InfraFailed;
                        }
                    }
                    if let Some(r) = reason
                        && existing.project_v1().result.status != EvalResultStatus::Completed
                    {
                        existing.failure_reason = Some(r);
                    }
                    *existing_v1 = existing.project_v1();
                } else {
                    return Err(StoreError::Integrity(format!(
                        "post-hoc recovery references unknown attempt {cell_id}#{sequence}"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Exclusive active writer session holding `.writer.lock`, an append-only handle
/// to `journal.jsonl`, and the validated in-memory journal state.
pub struct AttemptStoreWriter<'a> {
    _store: &'a FileAttemptStore,
    _guard: WriterGuard,
    file: File,
    state: ReconstructedStoreState,
}

impl AttemptStoreWriter<'_> {
    pub fn state(&self) -> &ReconstructedStoreState {
        &self.state
    }

    pub fn next_sequence(&self, cell_id: &str) -> u32 {
        self.state.next_sequence(cell_id)
    }

    pub fn list_v1(&self, cell_id: &str) -> Vec<V1CellAttempt> {
        self.state
            .settled_v1
            .get(cell_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Validates `entry` against the in-memory state machine, appends it to `journal.jsonl`,
    /// fsyncs the frame, and then advances the in-memory state.
    pub fn append_journal_entry(&mut self, entry: &JournalEntry) -> Result<(), StoreError> {
        let mut next = self.state.clone();
        next.apply_entry(entry.clone())?;
        let mut frame = serde_json::to_vec(entry)?;
        frame.push(b'\n');
        self.file.write_all(&frame)?;
        self.file.sync_all()?;
        self.state = next;
        Ok(())
    }

    /// Records a pre-dispatch `LaunchIntent` before any external resource or Host operation starts.
    pub fn record_launch_intent(
        &mut self,
        cell_id: &str,
        sequence: u32,
        started_at: u64,
        intent_id: &str,
    ) -> Result<(), StoreError> {
        self.append_journal_entry(&JournalEntry::LaunchIntent {
            cell_id: cell_id.to_owned(),
            sequence,
            started_at,
            intent_id: intent_id.to_owned(),
        })
    }

    pub fn record_resource_tagged(
        &mut self,
        cell_id: &str,
        sequence: u32,
        resource_kind: &str,
        resource_id: &str,
        operation_id: Option<String>,
        invocation_id: Option<String>,
    ) -> Result<(), StoreError> {
        self.append_journal_entry(&JournalEntry::ResourceTagged {
            cell_id: cell_id.to_owned(),
            sequence,
            resource_kind: resource_kind.to_owned(),
            resource_id: resource_id.to_owned(),
            operation_id,
            invocation_id,
        })
    }

    pub fn record_checkpoint(
        &mut self,
        cell_id: &str,
        sequence: u32,
        recorded_at: u64,
        metering: AttemptMetering,
    ) -> Result<(), StoreError> {
        self.append_journal_entry(&JournalEntry::Checkpoint {
            cell_id: cell_id.to_owned(),
            sequence,
            recorded_at,
            metering,
        })
    }

    /// Commits a settled record to the sole durable authority: the journal.
    pub fn append_v2(&mut self, record: &V2AttemptRecord) -> Result<V1CellAttempt, StoreError> {
        self.append_journal_entry(&JournalEntry::Settled {
            attempt: Box::new(record.clone()),
        })?;
        Ok(self
            .state
            .settled_v1
            .get(&record.cell_id)
            .and_then(|v| v.last())
            .expect("Settled journal entry produces a v1 projection")
            .clone())
    }

    /// Appends a `maka.eval.v1` attempt directly (for legacy compatibility tests and stores)
    /// while synthesizing the corresponding `v2` record in the journal.
    pub fn append_v1(&mut self, attempt: &V1CellAttempt) -> Result<(), StoreError> {
        let validated = decode_v1_cell_attempt(&serde_json::to_value(attempt)?)?;
        let v2 = v2_from_v1(&validated);
        self.append_v2(&v2)?;
        Ok(())
    }

    /// Appends a post-hoc recovery entry without mutating any existing attempt file or journal entry.
    pub fn record_post_hoc_recovery(
        &mut self,
        cell_id: &str,
        sequence: u32,
        recovered_at: u64,
        metering: Option<AttemptMetering>,
        cleanup_confirmed: bool,
        reason: Option<String>,
    ) -> Result<(), StoreError> {
        self.append_journal_entry(&JournalEntry::PostHocRecovery {
            cell_id: cell_id.to_owned(),
            sequence,
            recovered_at,
            metering,
            cleanup_confirmed,
            reason,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FileAttemptStore {
    root: PathBuf,
    journal_path: PathBuf,
}

impl FileAttemptStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let journal_path = root.join("journal.jsonl");
        Self { root, journal_path }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub fn acquire_writer(&self) -> Result<AttemptStoreWriter<'_>, StoreError> {
        let guard = WriterGuard::acquire(&self.root)?;
        self.recover_uncommitted_tail()?;
        let state = self.reconstruct_state()?;
        let mut options = OpenOptions::new();
        options.create(true).append(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&self.journal_path)?;
        Ok(AttemptStoreWriter {
            _store: self,
            _guard: guard,
            file,
            state,
        })
    }

    pub fn run_exclusive<T, F>(&self, operation: F) -> Result<T, StoreError>
    where
        F: FnOnce(&mut AttemptStoreWriter<'_>) -> Result<T, StoreError>,
    {
        let mut writer = self.acquire_writer()?;
        operation(&mut writer)
    }

    /// Truncates any uncommitted trailing fragment (without a terminating `\n`) left
    /// behind by an interrupted append, while rejecting corruption in committed frames.
    fn recover_uncommitted_tail(&self) -> Result<(), StoreError> {
        let mut file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.journal_path)
        {
            Ok(f) => f,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(StoreError::Io(err)),
        };
        let len = file.metadata()?.len();
        if len == 0 {
            return Ok(());
        }
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        file.read_exact(&mut last)?;
        if last[0] == b'\n' {
            return Ok(());
        }

        file.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(&file);
        let mut committed_len = 0u64;
        let mut raw_line = Vec::new();
        loop {
            raw_line.clear();
            let n = reader.read_until(b'\n', &mut raw_line)?;
            if n == 0 {
                break;
            }
            if !raw_line.ends_with(b"\n") {
                break;
            }
            let trimmed = raw_line.trim_ascii();
            if !trimmed.is_empty() {
                let _: JournalEntry = serde_json::from_slice(trimmed)?;
            }
            committed_len = committed_len.saturating_add(raw_line.len() as u64);
        }
        drop(reader);
        file.set_len(committed_len)?;
        file.sync_all()?;
        Ok(())
    }

    /// Projects only committed journal records, including post-hoc evidence.
    pub fn list_v1(&self, cell_id: &str) -> Result<Vec<V1CellAttempt>, StoreError> {
        Ok(self
            .reconstruct_state()?
            .settled_v1
            .remove(cell_id)
            .unwrap_or_default())
    }

    pub fn read_journal_entries(&self) -> Result<Vec<JournalEntry>, StoreError> {
        let file = match File::open(&self.journal_path) {
            Ok(f) => f,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(StoreError::Io(err)),
        };
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut raw_line = Vec::new();
        loop {
            raw_line.clear();
            let n = reader.read_until(b'\n', &mut raw_line)?;
            if n == 0 {
                break;
            }
            // `\n` is the single-frame commit boundary; an unterminated trailing
            // fragment indicates an interrupted append rather than committed corruption.
            if !raw_line.ends_with(b"\n") {
                break;
            }
            let trimmed = raw_line.trim_ascii();
            if trimmed.is_empty() {
                continue;
            }
            let entry: JournalEntry = serde_json::from_slice(trimmed)?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Deterministically reconstructs all settled `v2` records, `v1` projections, and
    /// unsettled in-flight attempts from the append-only journal.
    pub fn reconstruct_state(&self) -> Result<ReconstructedStoreState, StoreError> {
        let entries = self.read_journal_entries()?;
        let mut state = ReconstructedStoreState::default();
        for entry in entries {
            state.apply_entry(entry)?;
        }
        Ok(state)
    }

    pub fn next_sequence(&self, cell_id: &str) -> Result<u32, StoreError> {
        Ok(self.reconstruct_state()?.next_sequence(cell_id))
    }
}

#[derive(Debug, Clone)]
pub struct ExperimentDirectory {
    pub root: PathBuf,
    pub spec_path: PathBuf,
    pub attempts: FileAttemptStore,
}

impl ExperimentDirectory {
    pub fn open(root: impl Into<PathBuf>, spec: &ExperimentSpec) -> Result<Self, StoreError> {
        let root = root.into();
        let raw_val = serde_json::to_value(spec)?;
        let validated_spec = parse_experiment_spec(&raw_val)?;
        let spec_val = serde_json::to_value(&validated_spec)?;
        fs::create_dir_all(&root)?;
        let spec_path = root.join("experiment.json");
        let canonical = format!("{}\n", canonical_json_string(&spec_val));
        let temp_path = root.join(format!(".experiment-{}.tmp", Uuid::new_v4()));

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut temp_file = options.open(&temp_path)?;
        let link_res = (|| -> Result<(), std::io::Error> {
            temp_file.write_all(canonical.as_bytes())?;
            temp_file.sync_all()?;
            drop(temp_file);
            fs::hard_link(&temp_path, &spec_path)
        })();
        let _ = fs::remove_file(&temp_path);

        match link_res {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing_raw = fs::read_to_string(&spec_path)?;
                let existing_val: Value = serde_json::from_str(&existing_raw)?;
                let existing_spec = parse_experiment_spec(&existing_val)?;
                let existing_canonical =
                    canonical_json_string(&serde_json::to_value(&existing_spec)?);
                if existing_canonical != canonical_json_string(&spec_val) {
                    return Err(StoreError::Integrity("experiment spec differs".into()));
                }
            }
            Err(err) => return Err(StoreError::Io(err)),
        }

        Ok(Self {
            spec_path,
            attempts: FileAttemptStore::new(root.join("attempts")),
            root,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapturedArtifactFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MakaRuntimeArtifactManifest {
    pub schema_version: String,
    pub reason: String,
    pub captured_at: u64,
    pub files: Vec<CapturedArtifactFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Verifies whether every required critical artifact path exists as a readable regular file.
///
/// Explicitly distinguishes a `0-byte` regular file (`Present`, valid empty file) from a
/// missing, symlink, directory, or unreadable path (`Missing`).
pub fn verify_critical_artifacts(
    base_dir: &Path,
    required_paths: &[&str],
) -> (CriticalArtifactsState, Vec<CapturedArtifactFile>) {
    let mut missing = Vec::new();
    let mut captured = Vec::new();

    for rel in required_paths {
        let full = base_dir.join(rel);
        match describe_regular_file(&full, rel) {
            Some(desc) => captured.push(desc),
            None => missing.push((*rel).to_owned()),
        }
    }

    if missing.is_empty() {
        (CriticalArtifactsState::Present, captured)
    } else {
        (CriticalArtifactsState::Missing { paths: missing }, captured)
    }
}

pub fn describe_regular_file(path: &Path, reported_name: &str) -> Option<CapturedArtifactFile> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(CapturedArtifactFile {
        path: reported_name.to_owned(),
        bytes: bytes.len() as u64,
        sha256: format!("sha256:{}", sha256_hex(&bytes)),
    })
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

/// Captures Maka runtime artifacts (`runtime.sqlite`, logs, and `manifest.json`) into
/// `destination_root` using atomic directory staging.
pub fn capture_maka_runtime_artifacts(
    state_root: &Path,
    destination_root: &Path,
    reason: &str,
    captured_at: u64,
) -> Result<MakaRuntimeArtifactManifest, StoreError> {
    let dest_name = destination_root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            StoreError::Integrity("artifact destination has no directory name".into())
        })?;
    let parent_dir = match destination_root.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if parent_dir != Path::new(".") {
        fs::create_dir_all(parent_dir)?;
    }
    let staging_root = parent_dir.join(format!(
        ".{dest_name}.tmp.{}.{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let _ = fs::remove_dir_all(&staging_root);
    fs::create_dir_all(&staging_root)?;

    let res = (|| -> Result<MakaRuntimeArtifactManifest, StoreError> {
        let mut errors = Vec::new();
        let sqlite_src = state_root.join("runtime.sqlite");
        let sqlite_dst = staging_root.join("runtime.sqlite");
        if is_regular_file(&sqlite_src) {
            let backup_res = (|| -> Result<(), String> {
                let src_conn = rusqlite::Connection::open_with_flags(
                    &sqlite_src,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .map_err(|e| format!("sqlite open failed: {e}"))?;
                src_conn
                    .busy_timeout(Duration::from_secs(5))
                    .map_err(|e| format!("sqlite busy_timeout failed: {e}"))?;
                let dst_str = sqlite_dst
                    .to_str()
                    .ok_or_else(|| "invalid sqlite destination path".to_owned())?;
                src_conn
                    .execute("VACUUM INTO ?1", [dst_str])
                    .map_err(|e| format!("sqlite backup failed: {e}"))?;
                Ok(())
            })();
            if let Err(err) = backup_res {
                errors.push(err);
                let _ = fs::remove_file(&sqlite_dst);
                if let Err(copy_err) = fs::copy(&sqlite_src, &sqlite_dst) {
                    errors.push(format!("sqlite fallback copy failed: {copy_err}"));
                    let _ = fs::remove_file(&sqlite_dst);
                }
            }
        }

        for candidate in [
            "runtime-host-candidate.log",
            "runtime-policy.json",
            "collection-error.json",
        ] {
            let src = state_root.join(candidate);
            if is_regular_file(&src)
                && let Err(err) = fs::copy(&src, staging_root.join(candidate))
            {
                errors.push(format!("failed to copy {candidate}: {err}"));
            }
        }

        let mut files = Vec::new();
        for name in [
            "runtime.sqlite",
            "runtime-host-candidate.log",
            "runtime-policy.json",
            "collection-error.json",
        ] {
            if let Some(desc) = describe_regular_file(&staging_root.join(name), name) {
                files.push(desc);
            }
        }

        let manifest = MakaRuntimeArtifactManifest {
            schema_version: "maka.eval.runtime_artifacts.v1".into(),
            reason: reason.to_owned(),
            captured_at,
            files,
            errors,
        };
        let manifest_bytes = format!("{}\n", serde_json::to_string_pretty(&manifest)?);
        fs::write(staging_root.join("manifest.json"), manifest_bytes)?;

        let backup_root = parent_dir.join(format!(
            ".{dest_name}.bak.{}.{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let had_existing = fs::symlink_metadata(destination_root).is_ok();
        if had_existing {
            fs::rename(destination_root, &backup_root)?;
        }
        if let Err(err) = fs::rename(&staging_root, destination_root) {
            if had_existing {
                let _ = fs::rename(&backup_root, destination_root);
            }
            return Err(StoreError::Io(err));
        }
        if had_existing {
            if fs::symlink_metadata(&backup_root).is_ok_and(|m| m.file_type().is_dir()) {
                let _ = fs::remove_dir_all(&backup_root);
            } else {
                let _ = fs::remove_file(&backup_root);
            }
        }
        if let Ok(dir) = File::open(parent_dir) {
            let _ = dir.sync_all();
        }
        Ok(manifest)
    })();

    if res.is_err() {
        let _ = fs::remove_dir_all(&staging_root);
    }
    res
}

pub fn write_atomic_bytes(final_path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let file_name = final_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| StoreError::Integrity("atomic write path has no file name".into()))?;
    let parent_dir = match final_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if parent_dir != Path::new(".") {
        fs::create_dir_all(parent_dir)?;
    }
    let temp_path = parent_dir.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_file = options.open(&temp_path)?;
    let res = (|| -> Result<(), StoreError> {
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);
        fs::rename(&temp_path, final_path)?;
        if let Ok(dir) = File::open(parent_dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    res
}

fn v2_from_v1(v1: &V1CellAttempt) -> V2AttemptRecord {
    let execution_outcome = match v1.result.status {
        EvalResultStatus::Completed => ExecutionOutcome::Completed,
        EvalResultStatus::SubjectFailed => ExecutionOutcome::SubjectFailed,
        EvalResultStatus::InfraFailed => ExecutionOutcome::InfraFailed,
        EvalResultStatus::Indeterminate => ExecutionOutcome::Indeterminate,
    };
    let verifier_outcome = match (v1.result.status, v1.result.score) {
        (EvalResultStatus::Completed, Some(score)) => VerifierOutcome::Verified { score },
        (EvalResultStatus::SubjectFailed, score) => VerifierOutcome::SubjectFailed {
            score,
            reason: v1.result.failure_reason.clone(),
        },
        _ => VerifierOutcome::Skipped,
    };
    let metering = match (&v1.result.usage, v1.result.cost_usd) {
        (Some(usage), Some(cost_usd)) => AttemptMetering::Complete {
            usage: usage.clone(),
            pricing: Some(crate::contract::PricingEvidence::SubjectReported { cost_usd }),
        },
        (Some(usage), None) => AttemptMetering::LowerBound {
            usage: usage.clone(),
        },
        (None, _) => AttemptMetering::Unknown,
    };
    V2AttemptRecord {
        schema_version: SchemaVersion::V2,
        cell_id: v1.cell_id.clone(),
        sequence: v1.sequence,
        started_at: v1.started_at,
        completed_at: v1.completed_at,
        duration_ms: v1.result.duration_ms,
        phase: AttemptPhase::Settled,
        execution_outcome,
        verifier_outcome,
        metering,
        cleanup_state: CleanupState::Confirmed,
        audit_state: EgressAuditState::NotRequired,
        critical_artifacts_state: CriticalArtifactsState::Present,
        cancellation: AttemptCancellation::None,
        recovered_post_hoc: false,
        failure_reason: v1.result.failure_reason.clone(),
        artifacts: v1.result.artifacts.clone(),
    }
}
