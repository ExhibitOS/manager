// SPDX-License-Identifier: Apache-2.0
//! Local trust administration and fenced source diagnostics/archive authentication.
use exhibitos_lifecycle::signed_release::{self, Policy};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};
fn open(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = options.open(path).map_err(|_| "UPDATE_INPUT_UNAVAILABLE")?;
    let m = f.metadata().map_err(|_| "UPDATE_INPUT_UNAVAILABLE")?;
    if !m.is_file() {
        return Err("UPDATE_INPUT_INVALID".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return Err("UPDATE_INPUT_INVALID".into());
        }
    }
    Ok(f)
}
fn bounded(path: &Path, limit: u64, policy: bool) -> Result<Vec<u8>, String> {
    let mut f = open(path)?;
    let m = f.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if policy && (m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0) {
            return Err("UPDATE_POLICY_UNTRUSTED".into());
        }
    }
    #[cfg(not(unix))]
    if policy {
        return Err("UPDATE_POLICY_PLATFORM_UNVERIFIED".into());
    }
    if m.len() > limit {
        return Err("UPDATE_INPUT_LIMIT".into());
    }
    let mut b = Vec::new();
    Read::by_ref(&mut f)
        .take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|_| "UPDATE_INPUT_UNAVAILABLE")?;
    if b.len() as u64 > limit {
        return Err("UPDATE_INPUT_LIMIT".into());
    }
    let after = f.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    let current = std::fs::symlink_metadata(path).map_err(|_| "UPDATE_INPUT_INVALID")?;
    if b.len() as u64 != m.len()
        || current.is_symlink()
        || !same(&m, &after)
        || !same(&after, &current)
    {
        return Err("UPDATE_INPUT_CHANGED".into());
    }
    Ok(b)
}
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (
            a.dev(),
            a.ino(),
            a.len(),
            a.mtime(),
            a.mtime_nsec(),
            a.ctime(),
            a.ctime_nsec(),
            a.nlink(),
        ) == (
            b.dev(),
            b.ino(),
            b.len(),
            b.mtime(),
            b.mtime_nsec(),
            b.ctime(),
            b.ctime_nsec(),
            b.nlink(),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}
fn main() {
    if let Err(code) = run() {
        println!("{}", serde_json::json!({"code":code}));
        std::process::exit(1);
    }
}
fn verify() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 8
        || a[1] != "verify"
        || a[2] != "--policy"
        || a[4] != "--release"
        || a[6] != "--artifact"
    {
        return Err("UPDATE_USAGE: exhibitos-update verify --policy <trusted policy> --release <signed envelope> --artifact <local artifact>".into());
    }
    let policy: Policy = serde_json::from_slice(&bounded(
        Path::new(&a[3]),
        signed_release::MAX_PAYLOAD as u64,
        true,
    )?)
    .map_err(|_| "UPDATE_POLICY_INVALID")?;
    let envelope = bounded(Path::new(&a[5]), signed_release::MAX_ENVELOPE as u64, false)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "UPDATE_CLOCK_UNVERIFIED")?
        .as_secs();
    let mut verified =
        signed_release::verify(&envelope, &policy, now).map_err(|e| e.code().to_string())?;
    let path = Path::new(&a[7]);
    if path.file_name().and_then(|n| n.to_str()) != Some(verified.release().artifact.name.as_str())
    {
        return Err("UPDATE_ARTIFACT_NAME_MISMATCH".into());
    }
    let mut artifact = open(path)?;
    let before = artifact.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    verified
        .verify_artifact(&mut artifact)
        .map_err(|e| e.code().to_string())?;
    let after = artifact.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    let current = std::fs::symlink_metadata(path).map_err(|_| "UPDATE_INPUT_INVALID")?;
    if current.is_symlink() || !same(&before, &after) || !same(&after, &current) {
        return Err("UPDATE_ARTIFACT_CHANGED".into());
    }
    let finished = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "UPDATE_CLOCK_UNVERIFIED")?
        .as_secs();
    signed_release::verify(&envelope, &policy, finished).map_err(|e| e.code().to_string())?;
    println!(
        "{}",
        serde_json::to_string(&verified.receipt()).map_err(|_| "UPDATE_RECEIPT_INVALID")?
    );
    Ok(())
}

fn now() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs())
        .map_err(|_| "UPDATE_CLOCK_UNVERIFIED".into())
}
fn policy(path: &str) -> Result<Policy, String> {
    serde_json::from_slice(&bounded(
        Path::new(path),
        signed_release::MAX_PAYLOAD as u64,
        true,
    )?)
    .map_err(|_| "UPDATE_POLICY_INVALID".into())
}
fn artifact(verified: &mut signed_release::VerifiedRelease, path: &Path) -> Result<(), String> {
    if path.file_name().and_then(|n| n.to_str()) != Some(verified.release().artifact.name.as_str())
    {
        return Err("UPDATE_ARTIFACT_NAME_MISMATCH".into());
    }
    let mut f = open(path)?;
    let before = f.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    verified
        .verify_artifact(&mut f)
        .map_err(|e| e.code().to_string())?;
    let after = f.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    let current = std::fs::symlink_metadata(path).map_err(|_| "UPDATE_INPUT_INVALID")?;
    if current.is_symlink() || !same(&before, &after) || !same(&after, &current) {
        return Err("UPDATE_ARTIFACT_CHANGED".into());
    }
    Ok(())
}
fn run() -> Result<(), String> {
    use signed_release::trust::Store;
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("verify") {
        return verify();
    }
    let code = |e: signed_release::Error| e.code().to_string();
    let usage = || {
        "UPDATE_USAGE: trust-provision|trust-policy|trust-status|accept|prepare-update|update-intent|discard-update-intent|execution-status|verify-update-backup|verify-update-source-stopped|verify-update-source-deployment|verify-update-source-configuration|verify-update-source-images|snapshot-update-source-database|verify-update-source-inventory|verify-update-configuration-inventory|prepare-update-candidate require --profile <absolute profile> --installation <default or UUID> and --apps-closed; see docs/release-trust.md".to_string()
    };
    if a.len() < 7
        || a[2] != "--profile"
        || a[4] != "--installation"
        || a.last().map(String::as_str) != Some("--apps-closed")
    {
        return Err(usage());
    }
    let profile = Path::new(&a[3]);
    let installation = &a[5];
    let result = match a[1].as_str() {
        "trust-provision" if a.len() == 9 && a[6] == "--policy" => {
            Store::provision(profile, installation, policy(&a[7])?, now()?)
                .map_err(code)?
                .receipt()
        }
        "trust-policy"
            if a.len() == 11 && a[6] == "--policy" && a[8] == "--expected-generation" =>
        {
            let mut s = Store::open(profile, installation).map_err(code)?;
            let expected = a[9].parse::<u64>().map_err(|_| usage())?;
            s.replace_policy(policy(&a[7])?, expected, now()?)
                .map_err(code)?
        }
        "trust-status" if a.len() == 7 => {
            Store::open(profile, installation).map_err(code)?.receipt()
        }
        "prepare-update"
            if a.len() == 13
                && a[6] == "--release"
                && a[8] == "--artifact"
                && a[10] == "--plan" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let envelope = bounded(Path::new(&a[7]), signed_release::MAX_ENVELOPE as u64, false)?;
            let plan: exhibitos_lifecycle::update::Plan = serde_json::from_slice(&bounded(
                Path::new(&a[11]),
                exhibitos_lifecycle::update::MAX_RECORD_BYTES as u64,
                true,
            )?)
            .map_err(|_| "UPDATE_PLAN_INVALID")?;
            let mut v = store
                .verify_for_preparation(&envelope, now()?)
                .map_err(code)?;
            artifact(&mut v, Path::new(&a[9]))?;
            let trust = store
                .prepare_update(&envelope, &v, plan, now()?)
                .map_err(code)?;
            println!(
                "{}",
                serde_json::json!({"trust":trust,"intent":store.intent(),"executed":false})
            );
            return Ok(());
        }
        "discard-update-intent"
            if a.len() == 12
                && a[6] == "--operation-id"
                && a[8] == "--expected-generation"
                && a[10] == "--preserve-data" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let expected = a[9].parse::<u64>().map_err(|_| usage())?;
            let trust = store
                .discard_prepared(&a[7], expected, now()?)
                .map_err(code)?;
            println!(
                "{}",
                serde_json::json!({"trust":trust,"intent":store.intent(),"executed":false,"dataPreserved":true})
            );
            return Ok(());
        }
        "prepare-update-candidate"
            if a.len() == 17
                && a[6] == "--maintenance-image"
                && a[8] == "--key"
                && a[10] == "--archive"
                && a[12] == "--port"
                && a[14] == "--fresh-candidate"
                && a[15] == "--external-writers-quiesced" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let port = a[13].parse::<u16>().map_err(|_| usage())?;
            let receipt = store
                .execution()
                .map_err(|e| e.code)?
                .prepare_target_candidate(&a[7], Path::new(&a[9]), Path::new(&a[11]), port, true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"restoration":receipt,"candidatePrepared":true,"updateExecuted":false,"preflightVerified":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "verify-update-source-stopped" if a.len() == 8 && a[6] == "--external-writers-quiesced" => {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .verify_source_stopped(true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"sourceStopped":true,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "verify-update-source-deployment"
            if a.len() == 8 && a[6] == "--external-writers-quiesced" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .verify_source_deployment(true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"hostDeploymentVerified":true,"dataInventoryVerified":false,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "verify-update-source-configuration"
            if a.len() == 10 && a[6] == "--image" && a[8] == "--external-writers-quiesced" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .verify_source_configuration(&a[7], true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"nativeFreezeKeyVerified":true,"configurationInventoryVerified":false,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "snapshot-update-source-database"
            if a.len() == 10 && a[6] == "--image" && a[8] == "--external-writers-quiesced" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .snapshot_source_database(&a[7], true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"physicalDatabaseCopied":true,"dataInventoryVerified":false,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "verify-update-source-inventory"
            if a.len() == 10 && a[6] == "--image" && a[8] == "--external-writers-quiesced" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .verify_source_inventory(&a[7], true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"dataInventoryVerified":true,"configurationInventoryVerified":false,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "verify-update-configuration-inventory"
            if a.len() == 10 && a[6] == "--image" && a[8] == "--external-writers-quiesced" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .verify_configuration_inventory(&a[7], true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"configurationInventoryVerified":true,"imageBytesVerified":true,"dataInventoryVerified":false,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "verify-update-source-images" if a.len() == 8 && a[6] == "--external-writers-quiesced" => {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let observation = store
                .execution()
                .map_err(|e| e.code)?
                .verify_source_images(true)
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"observation":observation,"currentImageMappingsVerified":true,"imageBytesVerified":false,"configurationInventoryVerified":false,"preflightVerified":false,"updateExecuted":false,"intent":store.intent()})
            );
            return Ok(());
        }
        "execution-status" if a.len() == 7 => {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let status = store
                .execution()
                .map_err(|e| e.code)?
                .source_status()
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"status":status,"updateExecuted":false})
            );
            return Ok(());
        }
        "verify-update-backup"
            if a.len() == 13
                && a[6] == "--maintenance-image"
                && a[8] == "--key"
                && a[10] == "--archive" =>
        {
            let mut store = Store::open(profile, installation).map_err(code)?;
            let receipt = store
                .execution()
                .map_err(|e| e.code)?
                .verify_backup(&a[7], Path::new(&a[9]), Path::new(&a[11]))
                .map_err(|e| e.code)?;
            println!(
                "{}",
                serde_json::json!({"verification":receipt,"updateExecuted":false,"restoreVerified":false})
            );
            return Ok(());
        }
        "update-intent" if a.len() == 7 => {
            let store = Store::open(profile, installation).map_err(code)?;
            println!(
                "{}",
                serde_json::json!({"trust":store.receipt(),"intent":store.intent(),"executed":false})
            );
            return Ok(());
        }
        "accept" if a.len() == 11 && a[6] == "--release" && a[8] == "--artifact" => {
            let mut s = Store::open(profile, installation).map_err(code)?;
            let envelope = bounded(Path::new(&a[7]), signed_release::MAX_ENVELOPE as u64, false)?;
            let mut v = s.verify(&envelope, now()?).map_err(code)?;
            artifact(&mut v, Path::new(&a[9]))?;
            let trust = s.accept(&envelope, &v, now()?).map_err(code)?;
            println!(
                "{}",
                serde_json::json!({"verification":v.receipt(),"trust":trust})
            );
            return Ok(());
        }
        _ => return Err(usage()),
    };
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| "UPDATE_RECEIPT_INVALID")?
    );
    Ok(())
}
