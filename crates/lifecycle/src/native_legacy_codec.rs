// SPDX-License-Identifier: Apache-2.0
//! Sole issuer for legacy authentication: fixed existing Platform codec, inherited
//! key handle, private quarantine, bounded result and held source/config fences.
use super::*;
use crate::{
    hosted_runtime_file::Pin,
    maintenance_source::{DirectoryPin, SourceSnapshot},
};
use std::{
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt},
            process::CommandExt,
        },
    },
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const RESERVE: u64 = 8 * 1024 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 16384;
const FILES: &[(&str, &[u8])] = &[
    (
        "scripts/native-legacy-verify.mjs",
        include_bytes!("../../../scripts/native-legacy-verify.mjs"),
    ),
    (
        "vendor/deployment/src/bounded-json.mjs",
        include_bytes!("../../../vendor/deployment/src/bounded-json.mjs"),
    ),
    (
        "vendor/deployment/LICENSE",
        include_bytes!("../../../vendor/deployment/LICENSE"),
    ),
    (
        "vendor/deployment/source.json",
        include_bytes!("../../../vendor/deployment/source.json"),
    ),
];
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigurationHash {
    bytes: u64,
    sha256: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Success {
    format: u32,
    operation: String,
    backup_id: String,
    files: u64,
    authenticated_manifest_sha256: String,
    archive_manifest_sha256: String,
    configuration: BTreeMap<String, ConfigurationHash>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Failure {
    error_code: String,
}
fn failed() -> LifecycleError {
    err("NATIVE_LEGACY_AUTHENTICATION_FAILED")
}
fn safe_error(code: &str) -> bool {
    matches!(
        code,
        "NATIVE_LEGACY_KEY_HANDLE_INVALID"
            | "NATIVE_LEGACY_INPUT_INVALID"
            | "NATIVE_LEGACY_RESULT_INVALID"
            | "NATIVE_LEGACY_RESOURCE_BUDGET"
            | "NATIVE_LEGACY_AUTHENTICATION_FAILED"
    )
}

pub(super) fn private_read(path: &Path, limit: u64) -> Result<(Secret, Pin)> {
    if fs::canonicalize(path).map_err(|_| invalid())? != path {
        return Err(invalid());
    }
    let (bytes, pin) = crate::hosted_runtime_file::bounded(path, limit)?;
    let bytes = Secret(bytes);
    let metadata = pin.duplicate_file()?.metadata().map_err(|_| invalid())?;
    if metadata.mode() & 0o7777 != 0o600 || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(invalid());
    }
    pin.check()?;
    Ok((bytes, pin))
}
fn resources(payload: &Payload) -> Result<(PathBuf, Vec<Pin>)> {
    let runtime = payload
        .script()
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(invalid)?;
    let root = runtime
        .parent()
        .ok_or_else(invalid)?
        .join("native-migration");
    if fs::canonicalize(&root).map_err(|_| err("NATIVE_LEGACY_CODEC_UNAVAILABLE"))? != root {
        return Err(invalid());
    }
    let mut pins = payload.legacy_codec_pins()?;
    for (name, expected) in FILES {
        let path = root.join(name);
        if fs::canonicalize(&path).map_err(|_| invalid())? != path {
            return Err(invalid());
        }
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
        if bytes != *expected {
            return Err(err("NATIVE_LEGACY_CODEC_UNAVAILABLE"));
        }
        pins.push(pin);
    }
    Ok((root.join("scripts/native-legacy-verify.mjs"), pins))
}
pub(crate) fn inspection_resources(payload: &Payload) -> Result<(PathBuf, Vec<Pin>)> {
    let (codec, mut pins) = resources(payload)?;
    let root = codec.parent().and_then(Path::parent).ok_or_else(invalid)?;
    for (name, expected) in [
        (
            "scripts/native-legacy-inspect.mjs",
            include_bytes!("../../../scripts/native-legacy-inspect.mjs").as_slice(),
        ),
        (
            "vendor/platform/migrations.mjs",
            include_bytes!("../../../vendor/platform/migrations.mjs").as_slice(),
        ),
        (
            "vendor/platform/migrations.ts",
            include_bytes!("../../../vendor/platform/migrations.ts").as_slice(),
        ),
        (
            "vendor/platform/LICENSE",
            include_bytes!("../../../vendor/platform/LICENSE").as_slice(),
        ),
        (
            "vendor/platform/native-migration-source.json",
            include_bytes!("../../../vendor/platform/native-migration-source.json").as_slice(),
        ),
    ] {
        let path = root.join(name);
        if fs::canonicalize(&path).map_err(|_| invalid())? != path {
            return Err(invalid());
        }
        let (bytes, pin) = crate::hosted_runtime_file::bounded(&path, 2 * 1024 * 1024)?;
        if bytes != expected {
            return Err(err("NATIVE_LEGACY_INSPECTION_UNAVAILABLE"));
        }
        pins.push(pin);
    }
    Ok((root.join("scripts/native-legacy-inspect.mjs"), pins))
}
struct OwnedChild(Child);
impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGTERM);
            }
        }
        let _ = self.0.wait();
    }
}
fn execute(
    payload: &Payload,
    script: &Path,
    request: &[u8],
    key: &Pin,
    cancel: &AtomicBool,
) -> Result<Success> {
    let held_key = key.duplicate_file()?;
    let descriptor = held_key.as_raw_fd();
    let mut command = Command::new(payload.node());
    command
        .arg(script)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Only async-signal-safe descriptor operations after fork. Node preads this
    // same regular key handle at offset zero, never reopens a /dev/fd alias.
    unsafe {
        command.pre_exec(move || {
            if descriptor != 3 && libc::dup2(descriptor, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let flags = libc::fcntl(3, libc::F_GETFD);
            if flags < 0 || libc::fcntl(3, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = OwnedChild(command.spawn().map_err(|_| failed())?);
    drop(held_key);
    let stdout = child.stdout.take().ok_or_else(failed)?;
    let capture = thread::spawn(move || {
        let mut stream = stdout;
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut invalid = false;
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if bytes.len() + n <= OUTPUT_LIMIT {
                        bytes.extend_from_slice(&chunk[..n]);
                    } else {
                        invalid = true;
                    }
                }
                Err(_) => {
                    invalid = true;
                    break;
                }
            }
        }
        (bytes, invalid)
    });
    let written = child.stdin.take().ok_or_else(failed)?.write_all(request);
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut stopping = false;
    let exit = loop {
        if !stopping
            && (written.is_err() || cancel.load(Ordering::SeqCst) || Instant::now() >= deadline)
        {
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            stopping = true;
        }
        match child.try_wait() {
            Ok(Some(exit)) => break Some(exit),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                stopping = true;
                break child.wait().ok();
            }
        }
    };
    let (mut bytes, overflow) = capture.join().map_err(|_| failed())?;
    if stopping || overflow || exit.is_none() {
        bytes.fill(0);
        return Err(failed());
    }
    if !exit.unwrap().success() {
        let failure = serde_json::from_slice::<Failure>(&bytes).ok();
        bytes.fill(0);
        return Err(err(failure
            .as_ref()
            .map(|f| f.error_code.as_str())
            .filter(|c| safe_error(c))
            .unwrap_or("NATIVE_LEGACY_AUTHENTICATION_FAILED")));
    }
    let result = serde_json::from_slice(&bytes).map_err(|_| failed());
    bytes.fill(0);
    result
}
fn frame_valid(frame: &Success, expected_cipher: &str) -> bool {
    frame.format == 1
        && frame.operation == "legacy-configuration-authenticated"
        && uuid::Uuid::parse_str(&frame.backup_id).is_ok_and(|v| v.to_string() == frame.backup_id)
        && (1..=100_000).contains(&frame.files)
        && hash_valid(&frame.authenticated_manifest_sha256)
        && frame.archive_manifest_sha256 == expected_cipher
        && frame
            .configuration
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == CONFIGURATION.into_iter().collect()
        && frame
            .configuration
            .values()
            .all(|v| (1..=1024 * 1024).contains(&v.bytes) && hash_valid(&v.sha256))
}
fn plaintext_name(path: &str) -> Result<String> {
    let name = path.strip_prefix("files/").ok_or_else(invalid)?;
    if name.len() != 12
        || !name.ends_with(".gcm")
        || !name.as_bytes()[..8].iter().all(u8::is_ascii_digit)
    {
        return Err(invalid());
    }
    Ok(format!("{name}.plain"))
}
fn verification_budget(cipher_bytes: u64, available: u64) -> Result<u64> {
    let budget = cipher_bytes
        .checked_mul(2)
        .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
        .ok_or_else(|| err("NATIVE_LEGACY_RESOURCE_BUDGET"))?;
    if budget
        .checked_add(RESERVE)
        .is_none_or(|needed| available < needed)
    {
        return Err(err("NATIVE_LEGACY_RESOURCE_BUDGET"));
    }
    Ok(budget)
}
/// All path inputs are selected inside the owner/controller layer, never an
/// 'authenticated' JSON object. This performs full existing-codec verification;
/// it does not restore a database, create an owner, activate, or issue grants.
pub(crate) fn authenticate(
    payload: &Payload,
    source: &Path,
    key_file: &Path,
    quarantine_parent: &Path,
    original_root: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<AuthenticatedLegacyConfig> {
    if cancel.load(Ordering::SeqCst) {
        return Err(failed());
    }
    let (script, runtime_pins) = resources(payload)?;
    crate::maintenance::input_path(source, true)?;
    crate::maintenance::input_path(key_file, false)?;
    if fs::canonicalize(source).map_err(|_| invalid())? != source
        || fs::canonicalize(key_file).map_err(|_| invalid())? != key_file
        || key_file.starts_with(source)
    {
        return Err(invalid());
    }
    crate::installations::private_directory(quarantine_parent)?;
    if fs::canonicalize(quarantine_parent).map_err(|_| invalid())? != quarantine_parent {
        return Err(invalid());
    }
    let archive = SourceSnapshot::capture(source)?;
    let cipher_bytes = archive.bounded_cipher_bytes()?;
    let write_budget = verification_budget(
        cipher_bytes,
        fs2::available_space(quarantine_parent).map_err(|_| invalid())?,
    )?;
    if original_root.is_some_and(|root| {
        !root.is_absolute() || fs::canonicalize(root).ok().as_deref() != Some(root)
    }) {
        return Err(invalid());
    }
    let quarantine_root =
        quarantine_parent.join(format!("native-legacy-auth-{}", uuid::Uuid::new_v4()));
    if quarantine_root.starts_with(source)
        || source.starts_with(&quarantine_root)
        || key_file.starts_with(&quarantine_root)
        || original_root.is_some_and(|root| {
            quarantine_root.starts_with(root) || root.starts_with(&quarantine_root)
        })
    {
        return Err(invalid());
    }
    let parent_pin = DirectoryPin::open(quarantine_parent)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(failed());
    }
    let (key_bytes, key) = private_read(key_file, 32)?;
    if key_bytes.0.len() != 32 {
        return Err(err("NATIVE_LEGACY_KEY_HANDLE_INVALID"));
    }
    drop(key_bytes);
    let (cipher, archive_manifest) =
        private_read(&source.join("manifest.gcm"), 16 * 1024 * 1024 + 36)?;
    let cipher_sha256 = crate::digest(&cipher.0);
    drop(cipher);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&quarantine_root)
        .map_err(|_| invalid())?;
    let quarantine_pin = DirectoryPin::open(&quarantine_root)?;
    let request = serde_json::to_vec(&serde_json::json!({"source":source,"workspace":quarantine_root,"archiveManifestSha256":cipher_sha256,"writeBudgetBytes":write_budget})).map_err(|_| invalid())?;
    if request.len() > OUTPUT_LIMIT {
        return Err(invalid());
    }
    archive.check()?;
    key.check()?;
    archive_manifest.check()?;
    parent_pin.check()?;
    quarantine_pin.check()?;
    for pin in &runtime_pins {
        pin.check()?;
    }
    let frame = execute(payload, &script, &request, &key, cancel)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(failed());
    }
    archive.check()?;
    key.check()?;
    archive_manifest.check()?;
    parent_pin.check()?;
    quarantine_pin.check()?;
    for pin in &runtime_pins {
        pin.check()?;
    }
    if !frame_valid(&frame, &cipher_sha256) {
        return Err(failed());
    }
    let verified = quarantine_root.join("verified");
    crate::installations::private_directory(&verified)?;
    let verified_pin = DirectoryPin::open(&verified)?;
    let (manifest_bytes, plaintext_manifest) =
        private_read(&verified.join("manifest.json"), 16 * 1024 * 1024)?;
    if crate::digest(&manifest_bytes.0) != frame.authenticated_manifest_sha256 {
        return Err(failed());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes.0).map_err(|_| invalid())?;
    if manifest["id"] != frame.backup_id
        || manifest["files"].as_array().map(Vec::len) != Some(frame.files as usize)
    {
        return Err(invalid());
    }
    let mut configuration = BTreeMap::new();
    let mut configuration_pins = Vec::new();
    for entry in manifest["files"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .filter(|e| e["role"] == "configuration")
    {
        let name = entry["name"]
            .as_str()
            .filter(|n| CONFIGURATION.contains(n))
            .ok_or_else(invalid)?;
        if configuration.contains_key(name) {
            return Err(invalid());
        }
        let plain = plaintext_name(entry["path"].as_str().ok_or_else(invalid)?)?;
        let (bytes, pin) = private_read(&verified.join(plain), 1024 * 1024)?;
        let expected = frame.configuration.get(name).ok_or_else(invalid)?;
        if bytes.0.len() as u64 != expected.bytes || crate::digest(&bytes.0) != expected.sha256 {
            return Err(failed());
        }
        configuration.insert(name.to_owned(), bytes);
        configuration_pins.push(pin);
    }
    let parsed = parse_configuration(&manifest_bytes.0, &configuration)?;
    let evidence = CodecEvidence {
        archive,
        key,
        key_path: key_file.into(),
        plaintext_manifest,
        configuration: configuration_pins,
        archive_root: source.into(),
        original_root: original_root.map(Path::to_path_buf),
        backup_id: frame.backup_id,
        authenticated_manifest_sha256: frame.authenticated_manifest_sha256,
        archive_manifest_sha256: cipher_sha256,
        archive_manifest,
        quarantine_parent: parent_pin,
        quarantine: quarantine_pin,
        verified_directory: verified_pin,
        quarantine_root,
    };
    evidence.check()?;
    Ok(AuthenticatedLegacyConfig {
        configuration: parsed,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> serde_json::Value {
        let configuration: BTreeMap<_, _> = CONFIGURATION
            .iter()
            .map(|name| {
                (
                    *name,
                    serde_json::json!({"bytes":128,"sha256":"a".repeat(64)}),
                )
            })
            .collect();
        serde_json::json!({"format":1,"operation":"legacy-configuration-authenticated","backupId":uuid::Uuid::new_v4().to_string(),"files":9,"authenticatedManifestSha256":"b".repeat(64),"archiveManifestSha256":"c".repeat(64),"configuration":configuration})
    }
    #[test]
    fn codec_frame_is_closed_and_bound_to_archive_and_exact_configuration() {
        let valid: Success = serde_json::from_value(frame()).unwrap();
        assert!(frame_valid(&valid, &"c".repeat(64)));
        assert!(!frame_valid(&valid, &"d".repeat(64)));
        let mut extra = frame();
        extra["authenticated"] = true.into();
        assert!(serde_json::from_value::<Success>(extra).is_err());
        let mut missing = frame();
        missing["configuration"]
            .as_object_mut()
            .unwrap()
            .remove("freeze-signing-key.json");
        assert!(!frame_valid(
            &serde_json::from_value(missing).unwrap(),
            &"c".repeat(64)
        ));
        let stale = serde_json::json!({"operation":"verified","files":9,"authenticatedManifestSha256":"b".repeat(64)});
        assert!(serde_json::from_value::<Success>(stale).is_err());
        // A valid frame alone is NOT a source capability: only authenticate()
        // can obtain it from its owned codec child and then recheck all pins.
    }
    #[test]
    fn quarantine_budget_keeps_floor_and_rejects_overflow_or_exhaustion() {
        let cipher = 64 * 1024 * 1024;
        assert_eq!(
            verification_budget(cipher, RESERVE + 2 * cipher).unwrap(),
            2 * cipher
        );
        assert!(verification_budget(cipher, RESERVE + 2 * cipher - 1).is_err());
        assert!(verification_budget(0, u64::MAX).is_err());
        assert!(verification_budget(u64::MAX, u64::MAX).is_err());
    }
    #[test]
    fn codec_plaintext_names_cannot_escape_private_quarantine() {
        assert_eq!(
            plaintext_name("files/00000007.gcm").unwrap(),
            "00000007.gcm.plain"
        );
        for name in [
            "../secret",
            "files/../secret",
            "files/123.gcm",
            "files/00000007.gcm/../secret",
            "files/00000007.gcm\0",
        ] {
            assert!(plaintext_name(name).is_err());
        }
        for code in [
            "/private/path",
            "NATIVE_LEGACY_AUTHENTICATION_FAILED\nSQL",
            "PRIVATE_TOKEN",
        ] {
            assert!(!safe_error(code));
        }
    }

    /// Real existing-codec execution against an explicitly supplied, retained
    /// synthetic LEGACY backup. Never starts a service or activates a target.
    #[test]
    #[ignore = "Requires an owned synthetic legacy archive/key, admitted payload/capsule and fresh private workspace; one global heavy owner"]
    fn actual_legacy_codec_issues_plan_and_refuses_wrong_key_and_tamper() {
        use std::os::unix::fs::PermissionsExt;
        fn input(name: &str) -> PathBuf {
            let path =
                PathBuf::from(std::env::var_os(name).expect("explicit synthetic fixture input"));
            assert!(path.is_absolute());
            assert!(fs::canonicalize(&path).unwrap() == path);
            path
        }
        fn file_hash(path: &Path) -> String {
            use sha2::{Digest, Sha256};
            let mut file = fs::File::open(path).unwrap();
            let mut hash = Sha256::new();
            let mut chunk = [0u8; 65536];
            loop {
                let n = file.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                hash.update(&chunk[..n]);
            }
            format!("{:x}", hash.finalize())
        }
        fn inventory(root: &Path) -> BTreeMap<PathBuf, String> {
            let mut result = BTreeMap::new();
            for entry in fs::read_dir(root).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                assert!(!meta.is_symlink());
                if meta.is_dir() {
                    for (relative, hash) in inventory(&path) {
                        result.insert(
                            PathBuf::from(path.file_name().unwrap()).join(relative),
                            hash,
                        );
                    }
                } else {
                    assert!(meta.is_file());
                    result.insert(path.file_name().unwrap().into(), file_hash(&path));
                }
            }
            result
        }
        let payload = Payload::open(&input("EXHIBITOS_LEGACY_TEST_PAYLOAD")).unwrap();
        let source = input("EXHIBITOS_LEGACY_TEST_ARCHIVE");
        let key = input("EXHIBITOS_LEGACY_TEST_KEY");
        let original = input("EXHIBITOS_LEGACY_TEST_ORIGINAL");
        let workspace = input("EXHIBITOS_LEGACY_TEST_WORKSPACE");
        let signing_witness = input("EXHIBITOS_LEGACY_TEST_SIGNING_WITNESS");
        crate::installations::private_directory(&workspace).unwrap();
        assert_eq!(fs::read_dir(&workspace).unwrap().count(), 0);
        assert!(!workspace.starts_with(&original) && !workspace.starts_with(&source));
        let expected_id = std::env::var("EXHIBITOS_LEGACY_TEST_BACKUP_ID").unwrap();
        let expected_sha = std::env::var("EXHIBITOS_LEGACY_TEST_MANIFEST_SHA256").unwrap();
        assert!(uuid::Uuid::parse_str(&expected_id).is_ok() && hash_valid(&expected_sha));
        let archive_before = inventory(&source);
        let key_before = file_hash(&key);
        let environment_path = original.join("runtime.env");
        let environment_before = file_hash(&environment_path);
        let signing_witness_before = file_hash(&signing_witness);
        let (original_signing, signing_witness_pin) =
            private_read(&signing_witness, 1024 * 1024).unwrap();
        let (original_environment, environment_pin) =
            private_read(&environment_path, 8192).unwrap();
        let cancel = AtomicBool::new(false);
        let authenticated = authenticate(
            &payload,
            &source,
            &key,
            &workspace,
            Some(&original),
            &cancel,
        )
        .unwrap();
        assert!(authenticated.evidence.backup_id == expected_id);
        assert!(authenticated.evidence.authenticated_manifest_sha256 == expected_sha);
        let origin = authenticated.configuration.original_origin.clone();
        let original_credentials =
            publication_source::parse(&original_environment.0, origin.clone()).unwrap();
        let config = &authenticated.configuration;
        assert!(config.credentials.tenant_id == original_credentials.tenant_id);
        assert!(config.credentials.subject == original_credentials.subject);
        assert!(config.credentials.password() == original_credentials.password());
        assert!(config.environment.0 == original_environment.0);
        assert_eq!(config.configuration_sha256.len(), 7);
        let signing_before = crate::digest(&config.freeze_signing_key.0);
        assert!(config.freeze_signing_key.0 == original_signing.0);
        let web = config.manifest.ports[0];
        let ports = Ports {
            web,
            api: if web == 13201 { 13202 } else { 13201 },
            database: 15432,
        };
        let target = workspace.join("new-native-target");
        let plan = authenticated
            .into_plan(target.clone(), &payload, ports)
            .unwrap();
        plan.recheck().unwrap();
        assert!(!target.exists());
        assert!(plan.summary.owner_kind == "legacy_account");
        assert!(plan.summary.original_origin == origin);
        assert!(
            plan.summary.requires_original_writer_handoff
                && plan
                    .summary
                    .requires_database_object_and_freeze_verification
        );
        assert!(crate::digest(&plan.source.configuration.freeze_signing_key.0) == signing_before);
        let target_credentials =
            publication_source::parse(&plan.target_environment.0, origin).unwrap();
        assert!(target_credentials.tenant_id == original_credentials.tenant_id);
        assert!(target_credentials.subject == original_credentials.subject);
        assert!(target_credentials.password() == original_credentials.password());
        // An earlier successful receipt/plan never authorizes this other key.
        let wrong = workspace.join("wrong-synthetic-key");
        let mut wrong_bytes = Secret(vec![0; 32]);
        std::io::Read::read_exact(
            &mut fs::File::open("/dev/urandom").unwrap(),
            &mut wrong_bytes.0,
        )
        .unwrap();
        fs::write(&wrong, &wrong_bytes.0).unwrap();
        fs::set_permissions(&wrong, fs::Permissions::from_mode(0o600)).unwrap();
        let wrong_code = match authenticate(
            &payload,
            &source,
            &wrong,
            &workspace,
            Some(&original),
            &cancel,
        ) {
            Err(error) => error.code,
            Ok(_) => panic!("wrong key issued legacy authority"),
        };
        assert_eq!(wrong_code, "NATIVE_LEGACY_AUTHENTICATION_FAILED");
        let damaged = workspace.join("tampered-synthetic-archive");
        fs::create_dir(&damaged).unwrap();
        fs::set_permissions(&damaged, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(damaged.join("files")).unwrap();
        fs::set_permissions(damaged.join("files"), fs::Permissions::from_mode(0o700)).unwrap();
        for relative in archive_before.keys() {
            fs::copy(source.join(relative), damaged.join(relative)).unwrap();
        }
        // One authenticated ciphertext byte in a fresh copy, never the original.
        let damaged_file = damaged.join("files/00000000.gcm");
        use std::io::{Seek, SeekFrom};
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&damaged_file)
            .unwrap();
        let offset = file.metadata().unwrap().len() - 1;
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut byte = [0];
        file.read_exact(&mut byte).unwrap();
        byte[0] ^= 1;
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(&byte).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let tamper_code = match authenticate(
            &payload,
            &damaged,
            &key,
            &workspace,
            Some(&original),
            &cancel,
        ) {
            Err(error) => error.code,
            Ok(_) => panic!("tampered ciphertext issued legacy authority"),
        };
        assert_eq!(tamper_code, "NATIVE_LEGACY_AUTHENTICATION_FAILED");
        plan.recheck().unwrap();
        environment_pin.check().unwrap();
        signing_witness_pin.check().unwrap();
        assert!(file_hash(&signing_witness) == signing_witness_before);
        assert!(inventory(&source) == archive_before);
        assert!(file_hash(&key) == key_before);
        assert!(file_hash(&environment_path) == environment_before);
        assert!(!target.exists());
        let result = serde_json::json!({"scope":"ACTUAL_LEGACY_CODEC_ISSUER_ONLY_NOT_DB_STAGE_ACTIVATION_GUI_OR_RELEASE","opaqueIssueAndPlan":true,"sameLegacyAccountPasswordSigningAndOrigin":true,"wrongKeyRefusedDespitePriorSuccess":true,"tamperedCiphertextRefused":true,"wrongKeyCode":wrong_code,"tamperCode":tamper_code,"originalArchiveKeyAndEnvironmentUnchanged":true,"retainedOriginalSigningWitnessMatchedAndUnchanged":true,"targetNotCreated":true,"archiveFiles":archive_before.len(),"backupId":expected_id,"manifestSha256":expected_sha});
        let receipt = workspace.join("result.json");
        fs::write(&receipt, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        fs::set_permissions(receipt, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
