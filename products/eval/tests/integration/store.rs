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

use crate::support::sample_two_arm_spec_json;
use maka_eval::contract::{
    AttemptCancellation, AttemptMetering, AttemptPhase, CleanupState, CriticalArtifactsState,
    EgressAuditState, ExecutionOutcome, PricingEvidence, SchemaVersion, V1CellAttempt,
    V2AttemptRecord, VerifierOutcome, parse_experiment_spec,
};
use maka_eval::measurement::{MeteringState, NormalizedUsage};
use maka_eval::store::{
    ExperimentDirectory, FileAttemptStore, capture_maka_runtime_artifacts,
    verify_critical_artifacts,
};
use serde_json::json;
use std::fs;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn writer_lock_is_released_after_process_exit() {
    const KEY: &str = "MAKA_EVAL_TEST_CRASH_LOCK";
    if let Some(path) = std::env::var_os(KEY) {
        let store = FileAttemptStore::new(path);
        let _guard = store.acquire_writer().unwrap();
        std::process::exit(0);
    }
    let temp = TempDir::new().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::writer_lock_is_released_after_process_exit",
        ])
        .env(KEY, temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        temp.path().join(".writer.lock").is_file(),
        "child process must have acquired .writer.lock"
    );
    let store = FileAttemptStore::new(temp.path());
    let _guard = store
        .acquire_writer()
        .expect("crashed writer must release lock");
    assert!(store.acquire_writer().is_err());
}

#[test]
fn journal_torn_tail_recovers_committed_prefix_and_repairs_on_writer_acquire() {
    let temp = TempDir::new().unwrap();
    let store = FileAttemptStore::new(temp.path());
    {
        let mut writer = store.acquire_writer().unwrap();
        writer
            .record_launch_intent("task-alpha::1::maka", 1, 1000, "intent-1")
            .unwrap();
    }

    // Simulate a crash mid-write leaving an uncommitted JSON + invalid UTF-8 tail without `\n`.
    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(store.journal_path())
            .unwrap();
        f.write_all(br#"{"kind":"checkpoint","cell_id":"task-alpha::1::maka","sequence":1,"bad":"#)
            .unwrap();
        f.write_all(&[0xF0, 0x9F]).unwrap();
        f.sync_all().unwrap();
    }

    // Read-only recovery preserves the durable LaunchIntent and sequence allocation.
    let reopened = FileAttemptStore::new(temp.path());
    let state = reopened.reconstruct_state().unwrap();
    assert!(
        state
            .unsettled
            .contains_key(&("task-alpha::1::maka".to_owned(), 1))
    );
    assert_eq!(reopened.next_sequence("task-alpha::1::maka").unwrap(), 2);

    // Acquiring the writer repairs the torn tail; subsequent checkpoint, PostHocRecovery fallback
    // (preserving LowerBound instead of Unknown), and settled `append_v2` stay consistent.
    {
        let mut writer = reopened.acquire_writer().unwrap();
        writer
            .record_checkpoint(
                "task-alpha::1::maka",
                1,
                1100,
                AttemptMetering::LowerBound {
                    usage: NormalizedUsage {
                        input_tokens: 100,
                        output_tokens: 20,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                        reasoning_tokens: 0,
                        total_tokens: 120,
                    },
                },
            )
            .unwrap();
        writer
            .record_post_hoc_recovery(
                "task-alpha::1::maka",
                1,
                1200,
                None,
                true,
                Some("process killed".into()),
            )
            .unwrap();
        writer
            .append_v2(&V2AttemptRecord {
                schema_version: SchemaVersion::V2,
                cell_id: "task-alpha::1::maka".into(),
                sequence: 2,
                started_at: 1300,
                completed_at: 1500,
                duration_ms: 200,
                phase: AttemptPhase::Settled,
                execution_outcome: ExecutionOutcome::Completed,
                verifier_outcome: VerifierOutcome::Verified { score: 1.0 },
                metering: AttemptMetering::Unknown,
                cleanup_state: CleanupState::Confirmed,
                audit_state: EgressAuditState::NotRequired,
                critical_artifacts_state: CriticalArtifactsState::Present,
                cancellation: AttemptCancellation::None,
                recovered_post_hoc: false,
                failure_reason: None,
                artifacts: Vec::new(),
            })
            .unwrap();
    }

    let final_state = FileAttemptStore::new(temp.path())
        .reconstruct_state()
        .unwrap();
    assert!(final_state.unsettled.is_empty());
    let settled = &final_state.settled_v2["task-alpha::1::maka"];
    assert_eq!(settled.len(), 2);
    assert!(settled[0].metering.usage().is_some());
    assert_eq!(settled[0].metering.state(), MeteringState::LowerBound);
    assert_eq!(settled[0].project_v1().result.cost_usd, None);
    assert_eq!(settled[1].sequence, 2);

    // A `\n`-terminated corrupt frame must fail closed on read and during tail recovery even with a
    // trailing torn fragment after it.
    let corrupt_dir = temp.path().join("corrupt-mid");
    let corrupt_store = FileAttemptStore::new(&corrupt_dir);
    {
        let mut writer = corrupt_store.acquire_writer().unwrap();
        writer
            .record_launch_intent("task-alpha::1::maka", 1, 1000, "intent-1")
            .unwrap();
    }
    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(corrupt_store.journal_path())
            .unwrap();
        f.write_all(b"{\"kind\":\"broken\"\n{\"kind\":\"torn")
            .unwrap();
        f.sync_all().unwrap();
    }
    assert!(corrupt_store.reconstruct_state().is_err());
    assert!(corrupt_store.acquire_writer().is_err());
}

#[test]
fn post_hoc_recovery_confluence_v1_round_trip_and_critical_artifact_checks() {
    let usage = NormalizedUsage {
        input_tokens: 100,
        output_tokens: 20,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 120,
    };

    // 1. PostHocRecovery 1-step vs 2-step confluence and recovered cost_usd projection.
    let temp = TempDir::new().unwrap();
    let one_step = FileAttemptStore::new(temp.path().join("one-step"));
    one_step
        .run_exclusive(|w| {
            w.record_launch_intent("t1::1::maka", 1, 100, "i1")?;
            w.record_post_hoc_recovery(
                "t1::1::maka",
                1,
                200,
                Some(AttemptMetering::Complete {
                    usage: usage.clone(),
                    pricing: Some(PricingEvidence::Model {
                        model: "deepseek-v4-flash".into(),
                        cost_usd: Some(0.0000352),
                    }),
                }),
                true,
                Some("crashed and cleaned up".into()),
            )
        })
        .unwrap();

    let two_step = FileAttemptStore::new(temp.path().join("two-step"));
    two_step
        .run_exclusive(|w| {
            w.record_launch_intent("t1::1::maka", 1, 100, "i1")?;
            w.record_post_hoc_recovery(
                "t1::1::maka",
                1,
                150,
                None,
                false,
                Some("cleanup pending".into()),
            )?;
            w.record_post_hoc_recovery(
                "t1::1::maka",
                1,
                200,
                Some(AttemptMetering::Complete {
                    usage: usage.clone(),
                    pricing: Some(PricingEvidence::Model {
                        model: "deepseek-v4-flash".into(),
                        cost_usd: Some(0.0000352),
                    }),
                }),
                true,
                Some("crashed and cleaned up".into()),
            )
        })
        .unwrap();

    let s1 = one_step.reconstruct_state().unwrap();
    let s2 = two_step.reconstruct_state().unwrap();
    assert_eq!(
        s1.settled_v2["t1::1::maka"][0].execution_outcome,
        ExecutionOutcome::InfraFailed
    );
    assert_eq!(
        s2.settled_v2["t1::1::maka"][0].execution_outcome,
        ExecutionOutcome::InfraFailed
    );
    assert_eq!(
        s2.settled_v1["t1::1::maka"][0].result.cost_usd,
        Some(0.0000352)
    );

    // 2. Closed state machine rejects out-of-order / duplicate / orphan transitions.
    let bad_store = FileAttemptStore::new(temp.path().join("bad-seq"));
    let mut writer = bad_store.acquire_writer().unwrap();
    writer
        .record_launch_intent("t1::1::maka", 1, 100, "i1")
        .unwrap();
    assert!(
        writer
            .record_launch_intent("t1::1::maka", 2, 101, "i2")
            .is_err()
    );
    assert!(
        writer
            .record_checkpoint("t1::1::maka", 2, 102, AttemptMetering::Unknown)
            .is_err()
    );
    assert!(
        writer
            .record_post_hoc_recovery("missing::1::maka", 1, 103, None, true, None)
            .is_err()
    );

    // 3. Lossless append_v1 -> list_v1 round-trip when SubjectFailed has failureReason: null,
    // plus ExperimentDirectory::open smoke check.
    let spec = parse_experiment_spec(&sample_two_arm_spec_json()).unwrap();
    let exp_dir = ExperimentDirectory::open(temp.path().join("exp-dir"), &spec).unwrap();
    let parsed_v1: V1CellAttempt = serde_json::from_value(json!({
        "cellId": "t1::1::maka",
        "sequence": 1,
        "startedAt": 10,
        "completedAt": 20,
        "result": {
            "score": null,
            "usage": null,
            "costUsd": null,
            "durationMs": 10,
            "status": "subject_failed",
            "failureReason": null,
            "artifacts": []
        }
    }))
    .unwrap();
    exp_dir
        .attempts
        .run_exclusive(|w| w.append_v1(&parsed_v1))
        .unwrap();
    assert_eq!(
        exp_dir.attempts.list_v1("t1::1::maka").unwrap(),
        vec![parsed_v1]
    );

    // 4. 0-byte regular file is Present (bytes == 0), whereas missing path is Missing.
    let artifacts_dir = temp.path().join("critical-artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap();
    fs::write(artifacts_dir.join("stderr.txt"), b"").unwrap();
    let (present_state, files) = verify_critical_artifacts(&artifacts_dir, &["stderr.txt"]);
    assert_eq!(present_state, CriticalArtifactsState::Present);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].bytes, 0);

    let (missing_state, _) =
        verify_critical_artifacts(&artifacts_dir, &["stderr.txt", "missing-transcript.json"]);
    assert_eq!(
        missing_state,
        CriticalArtifactsState::Missing {
            paths: vec!["missing-transcript.json".into()]
        }
    );
}

#[test]
fn sqlite_runtime_artifact_capture_writes_consistent_snapshot_and_manifest() {
    let temp = TempDir::new().unwrap();
    let state_root = temp.path().join("host-state");
    fs::create_dir_all(&state_root).unwrap();

    // Create a real SQLite database at state_root/runtime.sqlite
    let db_path = state_root.join("runtime.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute("CREATE TABLE receipts (id TEXT PRIMARY KEY)", [])
        .unwrap();
    conn.execute("INSERT INTO receipts (id) VALUES ('op-123')", [])
        .unwrap();
    drop(conn);
    fs::write(
        state_root.join("runtime-policy.json"),
        b"{\"mode\":\"eval\"}\n",
    )
    .unwrap();

    let dest_root = temp.path().join("captured-artifacts");
    let manifest =
        capture_maka_runtime_artifacts(&state_root, &dest_root, "settled", 123456).unwrap();
    assert_eq!(manifest.schema_version, "maka.eval.runtime_artifacts.v1");
    assert_eq!(manifest.files.len(), 2);
    assert!(dest_root.join("runtime.sqlite").is_file());
    assert!(dest_root.join("manifest.json").is_file());
}

#[test]
fn artifact_capture_preserves_logs_and_records_error_when_sqlite_is_corrupt() {
    let temp = TempDir::new().unwrap();
    let state_root = temp.path().join("host-state");
    fs::create_dir_all(&state_root).unwrap();
    fs::write(state_root.join("runtime.sqlite"), b"not-a-sqlite-file").unwrap();
    fs::write(
        state_root.join("runtime-policy.json"),
        b"{\"mode\":\"eval\"}\n",
    )
    .unwrap();

    let dest_root = temp.path().join("captured-artifacts");
    let manifest =
        capture_maka_runtime_artifacts(&state_root, &dest_root, "settled", 123456).unwrap();
    assert!(!manifest.errors.is_empty());
    assert!(dest_root.join("runtime-policy.json").is_file());
    assert!(dest_root.join("runtime.sqlite").is_file());
    assert!(dest_root.join("manifest.json").is_file());
}
