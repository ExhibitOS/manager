// SPDX-License-Identifier: Apache-2.0
//! Independent assertions for the selected fresh 044 Session probe.

use super::*;
use std::{collections::BTreeMap, os::unix::fs::MetadataExt, path::Component};

const ORIGINAL_SHA: &str = "e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6";
const OUTPUT_SHA: &str = "694a9cf81dfbf12fd0841ae45240c5ae0f1d7cb129d0ee122be7cff86c4625bc";
const ORIGINAL_BYTES: usize = 8_383_096;
const OUTPUT_BYTES: usize = 16_036_864;

pub(super) struct Proof {
    pub(super) binding: hayes::Binding,
    pub(super) db_stage_id: Uuid,
    directory: PathBuf,
    stages: BTreeMap<Uuid, Value>,
}

fn record(directory: &Path, name: &str) -> (Vec<u8>, Value) {
    let bytes = fs::read(directory.join(name)).unwrap();
    let value = serde_json::from_slice(&bytes).unwrap();
    (bytes, value)
}

fn id(value: &Value) -> Uuid {
    let text = value.as_str().unwrap();
    let uuid = Uuid::parse_str(text).unwrap();
    assert_eq!(uuid.to_string(), text);
    uuid
}

fn verify_bytes(path: &Path, size: usize, sha: &str) {
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.file_type().is_file());
    assert_eq!(metadata.len(), size as u64);
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len(), size);
    assert_eq!(digest(&bytes), sha);
}

pub(super) fn before_release(custody: &Path, staged: &Value) -> Proof {
    let (_, active) = record(custody, "active.json");
    let binding: hayes::Binding = serde_json::from_value(active["binding"].clone()).unwrap();
    let directory = custody.join(binding.attempt.to_string());
    let (_, held) = record(&directory, "operation-workers-held.json");
    let (_, reservation) = record(&directory, "operation-output-reservation.json");
    let db_stage_id = id(&staged["stageId"]);
    assert_eq!(id(&held["binding"]["jobId"]), binding.job);
    assert_eq!(id(&staged["jobId"]), binding.job);
    assert_eq!(id(&reservation["dbStageId"]), db_stage_id);
    assert_eq!(held["binding"], reservation["binding"]);
    assert_eq!(
        id(&reservation["output"]["stageId"]),
        id(&held["outputStageId"])
    );
    assert_eq!(staged["bytes"], OUTPUT_BYTES);
    assert_eq!(staged["sha256"], OUTPUT_SHA);

    let stages: BTreeMap<_, _> = fs::read_dir(directory.join("stage"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let stage =
                Uuid::parse_str(name.to_str().unwrap().strip_prefix(".oex-stage-").unwrap())
                    .unwrap();
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            assert!(metadata.file_type().is_file() && metadata.nlink() == 1);
            let bytes = fs::read(entry.path()).unwrap();
            assert_eq!(metadata.len(), bytes.len() as u64);
            (
                stage,
                serde_json::json!({
                    "id":stage,
                    "physical":{"dev":metadata.dev(),"ino":metadata.ino()},
                    "bytes":bytes.len(),
                    "sha256":digest(&bytes),
                }),
            )
        })
        .collect();
    assert_eq!(stages.len(), 4);
    assert_eq!(stages[&binding.attempt]["bytes"], ORIGINAL_BYTES);
    assert_eq!(stages[&binding.attempt]["sha256"], ORIGINAL_SHA);
    let output = &stages[&id(&held["outputStageId"])];
    assert_eq!(output["bytes"], OUTPUT_BYTES);
    assert_eq!(output["sha256"], OUTPUT_SHA);
    Proof {
        binding,
        db_stage_id,
        directory,
        stages,
    }
}

pub(super) fn verify_release(
    session: &mut Session,
    root: &Path,
    original_path: &Path,
    proof: &Proof,
    committed: &Value,
    recorded: &Value,
) {
    let directory = &proof.directory;
    assert!(fs::read_dir(directory.join("stage"))
        .unwrap()
        .next()
        .is_none());
    let (intent_bytes, intent) = record(directory, "operation-release-intent.json");
    let (completion_bytes, completion) = record(directory, "operation-release-completed.json");
    let (_, ack) = record(directory, "operation-release-recorded.json");
    let (_, observed) = record(directory, "operation-disposition-observed.json");
    let (ready_bytes, ready) = record(directory, "operation-ready-evidence.json");
    let scope = &intent["scope"];
    let expected = intent["stages"].as_array().unwrap();
    assert_eq!(expected.len(), 4);
    let returned: BTreeMap<_, _> = expected
        .iter()
        .map(|stage| (id(&stage["id"]), stage.clone()))
        .collect();
    assert_eq!(returned, proof.stages);
    assert_eq!(completion["stages"], intent["stages"]);
    assert_eq!(completion["scope"], *scope);
    assert_eq!(completion["intentSha256"], digest(&intent_bytes));
    assert_eq!(completion["activeReservationRetired"], true);
    assert_eq!(completion["originalCustodyLockClosed"], true);
    assert_eq!(scope["nativeEvidenceSha256"], digest(&ready_bytes));
    assert_eq!(scope["binding"], ready["binding"]);
    assert_eq!(scope["database"], ready["database"]);
    assert_eq!(scope["workerManifestSha256"], ready["workerManifestSha256"]);
    assert_eq!(scope["receiptSha256"], observed["receiptSha256"]);
    for key in [
        "binding",
        "nativeOwnerId",
        "runtimeManifestSha256",
        "readyFrameSha256",
        "dbStageId",
        "challenge",
        "receiptId",
    ] {
        assert_eq!(ack[key], scope[key]);
        assert_eq!(observed[key], scope[key]);
    }
    assert_eq!(ack["kind"], "hayes-release-recorded");
    assert_eq!(id(&scope["binding"]["attemptId"]), proof.binding.attempt);
    assert_eq!(id(&scope["nativeOwnerId"]), proof.binding.owner);
    assert_eq!(id(&scope["dbStageId"]), proof.db_stage_id);
    assert_eq!(id(&committed["jobId"]), proof.binding.job);
    assert_eq!(id(&recorded["jobId"]), proof.binding.job);
    assert_eq!(id(&recorded["attemptId"]), proof.binding.attempt);
    assert_eq!(committed["receiptId"], scope["receiptId"]);
    assert_eq!(committed["receiptSha256"], scope["receiptSha256"]);
    assert_eq!(
        recorded["completionFrameSha256"],
        ack["completionFrameSha256"]
    );

    let tenant = id(&scope["binding"]["tenantId"]);
    let receipt = id(&scope["receiptId"]);
    let challenge = id(&scope["challenge"]);
    let receipt_sha = scope["receiptSha256"].as_str().unwrap();
    let frame_sha = ack["completionFrameSha256"].as_str().unwrap();
    assert!(hash_valid(receipt_sha) && hash_valid(frame_sha));
    let sql = format!("SELECT r.database_system_identifier,r.database_oid,r.intent_sha256,r.completion_sha256,
            d.original_asset_id,d.display_asset_id,d.derivative_id,o.object_key
        FROM hayes_native_releases r
        JOIN hayes_original_dispositions d ON (d.tenant_id,d.id,d.attempt_id,d.job_id,d.stage_id)=(r.tenant_id,r.disposition_id,r.attempt_id,r.job_id,r.stage_id)
        JOIN import_jobs j ON (j.tenant_id,j.id,j.hayes_disposition_id)=(d.tenant_id,d.job_id,d.id)
        JOIN assets o ON (o.tenant_id,o.id)=(d.tenant_id,d.original_asset_id)
        JOIN assets a ON (a.tenant_id,a.id)=(d.tenant_id,d.display_asset_id)
        WHERE r.tenant_id='{tenant}' AND r.attempt_id='{attempt}' AND r.job_id='{job}'
          AND r.stage_id='{stage}' AND r.disposition_id='{receipt}' AND r.challenge='{challenge}'
          AND r.receipt_sha256='{receipt_sha}' AND d.receipt_sha256=r.receipt_sha256
          AND encode(sha256(d.receipt_canonical),'hex')=r.receipt_sha256
          AND r.completion_frame_sha256='{frame_sha}' AND encode(sha256(r.completion_canonical),'hex')=r.completion_frame_sha256
          AND j.state='approved' AND j.asset_id=a.id AND o.state='quarantine' AND a.state='approved'
          AND o.sha256='{ORIGINAL_SHA}' AND o.bytes={ORIGINAL_BYTES} AND a.sha256='{OUTPUT_SHA}' AND a.bytes={OUTPUT_BYTES}",
        attempt=proof.binding.attempt, job=proof.binding.job, stage=proof.db_stage_id);
    let row = session
        .query_database_before("exhibitos", &sql, Instant::now() + Duration::from_secs(10))
        .unwrap();
    let fields: Vec<_> = row.split('|').collect();
    let [system, oid, intent_sha, completion_sha, original_asset, display_asset, derivative, source_key] =
        fields.as_slice()
    else {
        panic!("expected one exact approved receipt and 044 release row");
    };
    assert_eq!(scope["database"]["systemIdentifier"], *system);
    assert_eq!(scope["database"]["databaseOid"], *oid);
    assert_eq!(*intent_sha, digest(&intent_bytes));
    assert_eq!(*completion_sha, digest(&completion_bytes));
    for asset in [original_asset, display_asset, derivative] {
        assert_eq!(Uuid::parse_str(asset).unwrap().to_string(), *asset);
    }
    assert_ne!(original_asset, display_asset);
    assert!(source_key.starts_with(&format!("{tenant}/")));
    assert!(Path::new(source_key)
        .components()
        .all(|part| matches!(part, Component::Normal(_))));
    verify_bytes(original_path, ORIGINAL_BYTES, ORIGINAL_SHA);
    verify_bytes(
        &root.join("blobs").join(source_key),
        ORIGINAL_BYTES,
        ORIGINAL_SHA,
    );
    verify_bytes(
        &root.join("blobs").join(format!(
            "{tenant}/hayes-output/{}/{OUTPUT_SHA}",
            proof.binding.attempt
        )),
        OUTPUT_BYTES,
        OUTPUT_SHA,
    );
    println!("HAYES_044_DATABASE_RELEASE receiptSha={receipt_sha} completionSha={completion_sha} system={system} databaseOid={oid} originalAsset={original_asset} displayAsset={display_asset} derivative={derivative} output={OUTPUT_SHA} bytes={OUTPUT_BYTES}");
}
