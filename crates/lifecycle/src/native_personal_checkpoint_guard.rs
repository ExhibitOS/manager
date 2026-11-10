// SPDX-License-Identifier: Apache-2.0
//! Read-only admission diagnostics on a verified, disposable PostgreSQL copy.
use super::*;
use crate::native_payload::{Catalog45StartupRecovery, catalog45_startup_recovery};
use std::{
    io::Write,
    os::unix::ffi::OsStringExt,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const DATABASE: &str = "native-database";
const QUERY: &str = include_str!("native_personal_checkpoint_guard.sql");
const RESPONSE_LIMIT: u64 = 16_384;

fn unavailable() -> LifecycleError {
    err("NATIVE_PRE_READY_GUARD_DIAGNOSTIC_UNAVAILABLE")
}

impl InstallationController {
    pub(crate) fn inspect_pre_ready_guard(
        &self,
        token: &str,
        target: &Payload,
        staging: &Path,
        key_path: &Path,
        failed_generation: &str,
        recovery_receipt_sha256: &str,
    ) -> Result<Value> {
        if !installations::uuid(failed_generation) || !hash_valid(recovery_receipt_sha256) {
            return Err(invalid());
        }
        self.with_transition_key(
            token,
            key_path,
            staging,
            None,
            |service, profile, document, key| {
                let guard = service.lock()?;
                service.check_prepare_guard(&guard)?;
                let receipt =
                    format!("native-personal-transition-recovery-{recovery_receipt_sha256}.gcm");
                let envelope = installation_backup::source_bytes(
                    &service.root,
                    &receipt,
                    ENVELOPE_LIMIT as u64,
                    true,
                )?;
                if digest(&envelope) != recovery_receipt_sha256 {
                    return Err(invalid());
                }
                let checkpoint = service.transition_checkpoint(&envelope, key)?;
                let pair = (
                    checkpoint.old_manifest.as_str(),
                    checkpoint.new_manifest.as_str(),
                );
                checkpoint.check_host(true)?;
                checkpoint.check_retained(None)?;
                if checkpoint.staged != staging
                    || target.context().manifest_sha256 != pair.1
                    || catalog45_startup_recovery(pair)
                        != Some(Catalog45StartupRecovery::PreReadyPreservation)
                    || checkpoint.saved_document_sha256.as_deref()
                        != Some(saved_document_digest(document)?.as_str())
                    || crate::native_runtime::catalog45_recovery_source_generation(
                        &service.root,
                        Some(Catalog45StartupRecovery::PreReadyPreservation),
                    )? != failed_generation
                {
                    return Err(invalid());
                }
                let transition = FurnishingTransition::from_manifests(pair).ok_or_else(invalid)?;
                let trial = checkpoint.furnishing.as_ref().ok_or_else(invalid)?;
                let writer = trial.hold_source(service, transition)?;
                let bindings = bindings(&checkpoint)?;
                if read_pending(&service.root)? != bindings.old_env.0
                    || inventory_with_limit(&service.root, false, checkpoint.inventory_limit())?
                        != checkpoint.files
                {
                    return Err(invalid());
                }

                let pointer = "native-current-run.json";
                let state = format!("native-runs/{failed_generation}/state.json");
                let files = [
                    (service.root.as_path(), pointer, 8192),
                    (service.root.as_path(), state.as_str(), 8192),
                    (
                        service.root.as_path(),
                        receipt.as_str(),
                        ENVELOPE_LIMIT as u64,
                    ),
                    (staging, ENVELOPE, ENVELOPE_LIMIT as u64),
                ];
                let runtime_hashes = || -> Result<Vec<String>> {
                    files
                        .iter()
                        .map(|(root, name, limit)| {
                            Ok(digest(&installation_backup::source_bytes(
                                root, name, *limit, true,
                            )?))
                        })
                        .collect()
                };
                let before_runtime = runtime_hashes()?;
                let before_journals = self.personal_transition_journal_digests(service, profile)?;
                let record = read(service)?;
                let mut fields = personal_environment_fields(&record, &bindings.old_env.0)?;
                let password = Bytes(
                    fields
                        .remove("POSTGRES_PASSWORD")
                        .ok_or_else(invalid)?
                        .into_bytes(),
                );
                for (_, value) in fields {
                    value.into_bytes().fill(0);
                }
                let scratch = Scratch::new()?;
                let result = observe_database(&scratch.0, target, &checkpoint, &password);
                scratch.remove()?;
                service.check_prepare_guard(&guard)?;
                trial.check_recovery_root(&service.root, &writer, transition, None)?;
                if runtime_hashes()? != before_runtime
                    || self.personal_transition_journal_digests(service, profile)?
                        != before_journals
                    || inventory_with_limit(&service.root, false, checkpoint.inventory_limit())?
                        != checkpoint.files
                {
                    return Err(invalid());
                }
                let mut observation = result?;
                let object = observation.as_object_mut().ok_or_else(unavailable)?;
                object.insert("failedGeneration".into(), failed_generation.into());
                object.insert(
                    "recoveryReceiptSha256".into(),
                    recovery_receipt_sha256.into(),
                );
                object.insert("sourcePreserved".into(), true.into());
                Ok(observation)
            },
        )
    }
}

struct Scratch(PathBuf, crate::maintenance_source::DirectoryPin);

impl Scratch {
    fn new() -> Result<Self> {
        let mut template = b"/private/tmp/exhibitos-guard.XXXXXX\0".to_vec();
        if unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) }.is_null() {
            return Err(unavailable());
        }
        template.pop();
        let path = PathBuf::from(std::ffi::OsString::from_vec(template));
        let pin = crate::maintenance_source::DirectoryPin::open(&path)?;
        Ok(Self(path, pin))
    }

    fn remove(&self) -> Result<()> {
        self.1.check()?;
        fs::remove_dir_all(&self.0).map_err(|_| unavailable())
    }
}

fn observe_database(
    scratch: &Path,
    target: &Payload,
    checkpoint: &Checkpoint,
    password: &Bytes,
) -> Result<Value> {
    let files = checkpoint
        .files
        .iter()
        .filter(|(path, _)| path.starts_with(DATABASE))
        .map(|(path, entry)| (path.clone(), entry.clone()))
        .collect::<BTreeMap<_, _>>();
    let max_bytes = checkpoint.inventory_limit();
    let required = files.values().map(|entry| entry.bytes).sum::<u64>() + 64 * 1024 * 1024;
    if fs2::available_space(scratch).map_err(|_| unavailable())? < required {
        return Err(err("NATIVE_PHYSICAL_CHECKPOINT_SPACE"));
    }
    let database = scratch.join(DATABASE);
    copy_entries(&checkpoint.staged, true, scratch, &files)?;
    if inventory_members_with_sync(scratch, false, max_bytes, &[DATABASE], File::sync_all)? != files
        || installation_backup::source_bytes(&database, "PG_VERSION", 32, true)?.strip_suffix(b"\n")
            != Some(b"18")
        || ["postmaster.pid", "standby.signal", "recovery.signal"]
            .iter()
            .any(|name| files.contains_key(&Path::new(DATABASE).join(name)))
    {
        return Err(unavailable());
    }
    // The scratch server must ignore copied server configuration.
    let automatic = database.join("postgresql.auto.conf");
    if files.contains_key(&Path::new(DATABASE).join("postgresql.auto.conf")) {
        fs::remove_file(&automatic).map_err(|_| unavailable())?;
    }
    write_private_new(&database, "postgresql.auto.conf", b"", 4096)?;
    write_private_new(
        scratch,
        "server.conf",
        b"default_transaction_read_only = on\njit = off\n",
        4096,
    )?;
    write_private_new(
        scratch,
        "pg_hba.conf",
        b"local exhibitos exhibitos scram-sha-256\n",
        4096,
    )?;
    write_private_new(scratch, "pg_ident.conf", b"", 4096)?;

    let postgres = target.postgres();
    let bin = postgres.parent().ok_or_else(unavailable)?;
    target.recheck()?;
    let control = command(bin.join("pg_controldata"), scratch)
        .arg(&database)
        .stdout(Stdio::piped())
        .output()
        .map_err(|_| unavailable())?;
    let identifier = std::str::from_utf8(&control.stdout)
        .map_err(|_| unavailable())?
        .lines()
        .find_map(|line| {
            line.strip_prefix("Database system identifier:")
                .map(str::trim)
        })
        .filter(|id| {
            control.status.success() && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
        })
        .ok_or_else(unavailable)?;
    let child = command(&postgres, scratch)
        .arg("-D")
        .arg(&database)
        .args(["-p", "5432", "-h", "", "-k"])
        .arg(scratch)
        .args(["-c", "unix_socket_permissions=0700"])
        .arg("-c")
        .arg(format!(
            "config_file={}",
            scratch.join("server.conf").display()
        ))
        .arg("-c")
        .arg(format!(
            "hba_file={}",
            scratch.join("pg_hba.conf").display()
        ))
        .arg("-c")
        .arg(format!(
            "ident_file={}",
            scratch.join("pg_ident.conf").display()
        ))
        .spawn()
        .map_err(|_| unavailable())?;
    let mut server = ScratchServer(child);
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !command(bin.join("pg_isready"), scratch)
            .arg("-h")
            .arg(scratch)
            .args([
                "-p",
                "5432",
                "-U",
                "exhibitos",
                "-d",
                "exhibitos",
                "-t",
                "1",
            ])
            .status()
            .is_ok_and(|status| status.success())
        {
            if Instant::now() >= deadline
                || server.0.try_wait().map_err(|_| unavailable())?.is_some()
            {
                return Err(unavailable());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let password = std::str::from_utf8(&password.0).map_err(|_| unavailable())?;
        let expected_ledger = target.workspace_furnishing_ledger(
            target
                .workspace_furnishing_catalog()?
                .ok_or_else(unavailable)?,
        )?;
        query(
            bin,
            scratch,
            checkpoint,
            password,
            identifier,
            &expected_ledger,
        )
    })();
    server.stop()?;
    result
}

fn command(program: impl AsRef<Path>, scratch: &Path) -> Command {
    let mut command = Command::new(program.as_ref());
    command
        .current_dir(scratch)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn query(
    bin: &Path,
    scratch: &Path,
    checkpoint: &Checkpoint,
    password: &str,
    identifier: &str,
    ledger: &str,
) -> Result<Value> {
    let output = scratch.join("result");
    let file = private_options().open(&output).map_err(|_| unavailable())?;
    let mut process = command(bin.join("psql"), scratch)
        .arg("-h")
        .arg(scratch)
        .args([
            "-p",
            "5432",
            "-U",
            "exhibitos",
            "-d",
            "exhibitos",
            "--quiet",
            "--no-password",
            "--no-psqlrc",
            "--tuples-only",
            "--no-align",
            "--set=ON_ERROR_STOP=1",
            "--file=-",
        ])
        .arg(format!("--set=workspace={}", checkpoint.workspace_id))
        .arg(format!("--set=tenant={}", checkpoint.tenant_id))
        .arg(format!("--set=principal={}", checkpoint.principal_id))
        .arg(format!("--set=uid={}", checkpoint.owner_uid))
        .env("PGPASSWORD", password)
        .env("PGCONNECT_TIMEOUT", "3")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(file))
        .spawn()
        .map_err(|_| unavailable())?;
    let supplied = process
        .stdin
        .take()
        .ok_or_else(unavailable)
        .and_then(|mut input| input.write_all(QUERY.as_bytes()).map_err(|_| unavailable()));
    let status = wait(&mut process, Duration::from_secs(15));
    if status.is_err() {
        let _ = process.kill();
        let _ = process.wait();
    }
    supplied?;
    if !status?.success() {
        return Err(unavailable());
    }
    let raw = installation_backup::source_bytes(scratch, "result", RESPONSE_LIMIT, true)?;
    let mut result: Value = serde_json::from_slice(&raw).map_err(|_| unavailable())?;
    let fields = result.as_object_mut().ok_or_else(unavailable)?;
    if fields.remove("ownerVerified") != Some(true.into())
        || fields
            .remove("systemIdentifier")
            .as_ref()
            .and_then(Value::as_str)
            != Some(identifier)
        || fields.remove("ledger").as_ref().and_then(Value::as_str) != Some(ledger)
    {
        return Err(unavailable());
    }
    Ok(result)
}

fn wait(child: &mut Child, timeout: Duration) -> Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| unavailable())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct ScratchServer(Child);

impl ScratchServer {
    fn stop(&mut self) -> Result<()> {
        if self.0.try_wait().map_err(|_| unavailable())?.is_none() {
            if unsafe { libc::kill(self.0.id() as i32, libc::SIGINT) } != 0 {
                return Err(unavailable());
            }
            if wait(&mut self.0, Duration::from_secs(20))?.success() {
                return Ok(());
            }
        }
        Err(unavailable())
    }
}

impl Drop for ScratchServer {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // Only this owned scratch PostgreSQL child receives a shutdown signal.
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGQUIT);
            }
            let _ = self.0.wait();
        }
    }
}
