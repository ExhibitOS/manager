// SPDX-License-Identifier: Apache-2.0
//! Encrypted archive verification through an explicitly trusted immutable local image.
//! Does not create backups, restore databases, activate services or delete old data.
use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationReceipt {
    pub id: String,
    pub operation: String,
    pub files: u64,
    pub image: String,
    pub authenticated_manifest_sha256: String,
    pub at: u64,
}
pub(crate) fn input_path(path: &Path, directory: bool) -> Result<()> {
    let text = path.to_str().ok_or_else(|| err("BACKUP_PATH_INVALID"))?;
    if !path.is_absolute()
        || text.len() > 2048
        || text.chars().any(|c| c.is_control() || c == ',')
        || fs::canonicalize(path).map_err(|_| err("BACKUP_PATH_INVALID"))? != path
    {
        return Err(err("BACKUP_PATH_INVALID"));
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| err("BACKUP_PATH_INVALID"))?;
    if metadata.file_type().is_symlink()
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file() || metadata.len() != 32
        }
    {
        return Err(err("BACKUP_PATH_INVALID"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0
            || !directory
                && (metadata.nlink() != 1 || metadata.permissions().mode() & 0o7777 != 0o600)
        {
            return Err(err("BACKUP_PRIVATE_PERMISSIONS"));
        }
    }
    #[cfg(windows)]
    {
        if directory {
            crate::windows_private::HostDirectory::inspect(path)?.check_private()?;
        } else {
            let file = crate::windows_private::HostFile::open(path)?;
            file.check_private()?;
            if file.metadata()?.len() != 32 {
                return Err(err("BACKUP_PATH_INVALID"));
            }
        }
    }
    Ok(())
}
fn receipt(
    output: &[u8],
    id: &str,
    image: &str,
    manifest_hash: &str,
) -> Result<VerificationReceipt> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Output {
        operation: String,
        files: u64,
    }
    let value: Output = serde_json::from_slice(output).map_err(|_| err("BACKUP_RESULT_INVALID"))?;
    if value.operation != "verified"
        || value.files == 0
        || value.files > 1000000
        || !hash_valid(manifest_hash)
    {
        return Err(err("BACKUP_RESULT_INVALID"));
    }
    Ok(VerificationReceipt {
        id: id.into(),
        operation: value.operation,
        files: value.files,
        image: image.into(),
        authenticated_manifest_sha256: manifest_hash.into(),
        at: now(),
    })
}
impl LifecycleService {
    /// Image is a trusted operator-selected local content ID, never an image tag or pull target.
    /// Result contains no key contents, archive paths, plaintext inventory or raw engine logs.
    pub fn verify_backup(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
    ) -> Result<VerificationReceipt> {
        let _lock = self.lock()?;
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        #[cfg(windows)]
        {
            let key = native_input_path(key, false)?;
            let source = native_input_path(source, true)?;
            self.verify_backup_windows(image, &key, &source)
        }
        #[cfg(not(windows))]
        {
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        input_path(&self.root, true)?;
        input_path(key, false)?;
        input_path(source, true)?;
        if key.starts_with(source)
            || source.starts_with(&self.root)
            || self.root.starts_with(source)
        {
            return Err(err("BACKUP_PATH_OVERLAP"));
        }
        let manifest = self.manifest()?;
        let installed: BundleManifest = read_json(&self.root.join("installed.json"))?;
        if serde_json::to_value(installed).ok() != serde_json::to_value(&manifest).ok() {
            return Err(err("BUNDLE_CHANGED"));
        }
        let engine = self.engine(&manifest, false)?;
        let inspected: Value = serde_json::from_slice(&run(
            &engine,
            &["image".into(), "inspect".into(), image.into()],
            None,
            15,
        )?)
        .map_err(|_| err("IMAGE_INTEGRITY"))?;
        if !inspected[0]["Id"]
            .as_str()
            .is_some_and(|value| same_local_image_id(value, image))
        {
            return Err(err("IMAGE_INTEGRITY"));
        }
        let id = Uuid::new_v4().to_string();
        let workspace = self.root.join(format!("backup-verification-{id}"));
        fs::create_dir(&workspace).map_err(|_| err("STATE_UNAVAILABLE"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&workspace, fs::Permissions::from_mode(0o700))
                .map_err(|_| err("STATE_UNAVAILABLE"))?;
        }
        let container = format!("exhibitos-verify-{id}");
        let label = format!("com.exhibitos.verification={id}");
        let mut args = vec![
            "run".into(),
            "--rm".into(),
            "--name".into(),
            container.clone(),
            "--label".into(),
            label,
            "--network".into(),
            "none".into(),
            "--read-only".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges:true".into(),
            "--tmpfs".into(),
            "/tmp:rw,nosuid,nodev,size=256m,mode=1777".into(),
        ];
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::metadata(&workspace).map_err(|_| err("STATE_UNAVAILABLE"))?;
            args.extend([
                "--user".into(),
                format!("{}:{}", metadata.uid(), metadata.gid()),
            ]);
        }
        args.extend([
            "--mount".into(),
            format!(
                "type=bind,source={},target=/backup-key,readonly",
                key.display()
            ),
            "--mount".into(),
            format!(
                "type=bind,source={},target=/backup-source,readonly",
                source.display()
            ),
            "--mount".into(),
            format!(
                "type=bind,source={},target=/backup-workspace",
                workspace.display()
            ),
            image.into(),
            "verify".into(),
            "--key-file".into(),
            "/backup-key".into(),
            "--source".into(),
            "/backup-source".into(),
            "--destination".into(),
            "/backup-workspace/plaintext".into(),
        ]);
        let output = run(&engine, &args, None, 3600);
        if output.is_err() {
            // A killed engine CLI may leave its child container alive. Remove only our exact label.
            if let Ok(bytes) = run(
                &engine,
                &[
                    "inspect".into(),
                    "--format".into(),
                    "{{index .Config.Labels \"com.exhibitos.verification\"}}".into(),
                    container.clone(),
                ],
                None,
                15,
            ) && String::from_utf8(bytes).is_ok_and(|value| value.trim() == id)
            {
                let _ = run(
                    &engine,
                    &["rm".into(), "--force".into(), container],
                    None,
                    30,
                );
            }
        }
        let outcome = output.and_then(|value| {
            let path = workspace.join("plaintext/manifest.json");
            let metadata = fs::symlink_metadata(&path).map_err(|_| err("BACKUP_RESULT_INVALID"))?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 16 * 1024 * 1024
            {
                return Err(err("BACKUP_RESULT_INVALID"));
            }
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(|_| err("BACKUP_RESULT_INVALID"))?
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| err("BACKUP_RESULT_INVALID"))?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err(err("BACKUP_RESULT_INVALID"));
            }
            receipt(&value, &id, image, &digest(&bytes))
        });
        match outcome {
            Ok(value) => {
                write_json(&workspace, "receipt.json", &value)?;
                Ok(value)
            }
            Err(_) => {
                write_json(
                    &workspace,
                    "failed.json",
                    &serde_json::json!({"id":id,"operation":"verification-failed","at":now()}),
                )?;
                Err(err("BACKUP_VERIFICATION_FAILED"))
            }
        }
        }
    }
}
#[cfg(windows)]
impl LifecycleService {
    fn verify_backup_windows(
        &self,
        image: &str,
        key: &Path,
        source: &Path,
    ) -> Result<VerificationReceipt> {
        use crate::backup_creation::windows_cipher_transport as transport;
        if !image.strip_prefix("sha256:").is_some_and(hash_valid) {
            return Err(err("BACKUP_IMAGE_INVALID"));
        }
        input_path(&self.root, true)?;
        input_path(key, false)?;
        input_path(source, true)?;
        if key.starts_with(source)
            || source.starts_with(&self.root)
            || self.root.starts_with(source)
        {
            return Err(err("BACKUP_PATH_OVERLAP"));
        }
        let key_guard = crate::windows_private::HostFile::open(key)?;
        key_guard.check_private()?;
        let source_guard = crate::windows_private::HostDirectory::inspect(source)?;
        source_guard.check_private()?;
        let manifest = self.manifest()?;
        let installed: BundleManifest = read_json(&self.root.join("installed.json"))?;
        if serde_json::to_value(installed).ok() != serde_json::to_value(&manifest).ok() {
            return Err(err("BUNDLE_CHANGED"));
        }
        let engine = self.engine(&manifest, false)?;
        if engine != "docker" {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        let info = backup_creation::inspected(
            &engine,
            &["info".into(), "--format".into(), "{{json .}}".into()],
        )?;
        if info["OperatingSystem"] != "Docker Desktop" || info["OSType"] != "linux" {
            return Err(err("BACKUP_PLATFORM_UNVERIFIED"));
        }
        backup_creation::local_image(&engine, image)?;
        let (input, limit) = transport::archive_inputs(&self.root, key, source)?;
        let id = Uuid::new_v4().to_string();
        let workspace = self.root.join(format!("backup-verification-{id}"));
        let _workspace_guard = restoration::directory(&workspace)?;
        let volume = format!("exhibitos-verify-work-{id}");
        let container = format!("exhibitos-verify-{id}");
        run(
            &engine,
            &[
                "volume".into(),
                "create".into(),
                "--label".into(),
                format!("com.exhibitos.verification={id}"),
                volume.clone(),
            ],
            None,
            30,
        )?;
        let check_volume = || -> Result<()> {
            let observed = backup_creation::inspected(
                &engine,
                &["volume".into(), "inspect".into(), volume.clone()],
            )?;
            if observed["Name"] != volume
                || observed["Driver"] != "local"
                || observed["Labels"]["com.exhibitos.verification"] != id
                || !(observed["Options"].is_null()
                    || observed["Options"]
                        .as_object()
                        .is_some_and(|o| o.is_empty()))
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            Ok(())
        };
        check_volume()?;
        write_json(
            &workspace,
            "work.json",
            &serde_json::json!({"format":1,"id":id,"image":image,"volume":volume,"container":container,"createdAt":now()}),
        )?;
        let script = transport::bootstrap_archive(
            r#"
import {main} from './scripts/service-backup.mjs';
import {open,lstat} from 'node:fs/promises';
import {constants} from 'node:fs';
import {createHash} from 'node:crypto';
const result=await main(['verify','--key-file','/key','--source','/archive','--destination','/work/plaintext']);
const path='/work/plaintext/manifest.json',file=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
let manifestHash;
try{
 const before=await file.stat({bigint:true});
 if(!before.isFile()||before.nlink!==1n||before.size>16777216n)throw Error('BACKUP_RESULT_INVALID');
 const hash=createHash('sha256');let count=0;
 for await(const chunk of file.createReadStream({autoClose:false})){count+=chunk.length;if(count>16777216)throw Error('BACKUP_RESULT_INVALID');hash.update(chunk);}
 const after=await file.stat({bigint:true}),named=await lstat(path,{bigint:true});
 if(BigInt(count)!==before.size||before.dev!==after.dev||before.ino!==after.ino||before.size!==after.size||before.mtimeNs!==after.mtimeNs||before.ctimeNs!==after.ctimeNs||before.mode!==after.mode||before.uid!==after.uid||before.gid!==after.gid||after.nlink!==1n||named.isSymbolicLink()||named.dev!==after.dev||named.ino!==after.ino)throw Error('BACKUP_RESULT_INVALID');
 manifestHash=hash.digest('hex');
}finally{await file.close();}
console.log(JSON.stringify({operation:result.operation,files:result.files,manifestHash}));
"#,
        );
        let args: Vec<String> = [
            "run",
            "--pull",
            "never",
            "--interactive",
            "--name",
            &container,
            "--label",
            &format!("com.exhibitos.verification={id}"),
            "--network",
            "none",
            "--user",
            "0:0",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges:true",
            "--pids-limit",
            "128",
            "--memory",
            "1536m",
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,size=256m,mode=1777",
            "--mount",
            &format!("type=volume,source={volume},target=/work"),
            "--entrypoint",
            "node",
            image,
            "--input-type=module",
            "-e",
            &script,
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let output = run_observed_stream(
            &engine,
            &args,
            None,
            3600,
            (None, None),
            Some((input, limit)),
            || Ok(()),
        );
        if output.is_err()
            && let Ok(helper) =
                backup_creation::inspected(&engine, &["inspect".into(), container.clone()])
            && helper["Name"] == format!("/{container}")
                    && helper["Config"]["Labels"]["com.exhibitos.verification"] == id
                    && helper["Image"] == image
                    && helper["Mounts"].as_array().is_some_and(|m| {
                        m.iter().any(|m| {
                            m["Type"] == "volume"
                                && m["Name"] == volume
                                && m["Destination"] == "/work"
                        })
                    })
                {
                    let _ = run(
                        &engine,
                        &[
                            "stop".into(),
                            "--time".into(),
                            "30".into(),
                            container.clone(),
                        ],
                        None,
                        60,
                    );
        }
        let outcome = output.and_then(|bytes| {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct NativeOutput {
                operation: String,
                files: u64,
                manifest_hash: String,
            }
            let result: NativeOutput =
                serde_json::from_slice(&bytes).map_err(|_| err("BACKUP_RESULT_INVALID"))?;
            let output = serde_json::to_vec(
                &serde_json::json!({"operation":result.operation,"files":result.files}),
            )
            .map_err(|_| err("BACKUP_RESULT_INVALID"))?;
            check_volume()?;
            key_guard.check_private()?;
            source_guard.check_private()?;
            let helper = backup_creation::inspected(&engine, &["inspect".into(), container])?;
            if helper["Name"] != format!("/exhibitos-verify-{id}")
                || helper["Config"]["Labels"]["com.exhibitos.verification"] != id
                || helper["Image"] != image
                || helper["State"]["Running"] != false
                || helper["State"]["ExitCode"] != 0
                || !helper["Mounts"].as_array().is_some_and(|m| {
                    m.iter().any(|m| {
                        m["Type"] == "volume" && m["Name"] == volume && m["Destination"] == "/work"
                    })
                })
            {
                return Err(err("OWNERSHIP_CONFLICT"));
            }
            receipt(&output, &id, image, &result.manifest_hash)
        });
        match outcome {
            Ok(value) => {
                write_json(&workspace, "receipt.json", &value)?;
                Ok(value)
            }
            Err(_) => {
                write_json(
                    &workspace,
                    "failed.json",
                    &serde_json::json!({"id":id,"operation":"verification-failed","at":now()}),
                )?;
                Err(err("BACKUP_VERIFICATION_FAILED"))
            }
        }
    }
}

/// Resolve ordinary Windows drive spelling only after native lexical ancestor
/// checks. A junction/alias must not silently become a different accepted input.
#[cfg(windows)]
pub(crate) fn native_input_path(path: &Path, directory: bool) -> Result<PathBuf> {
    let text = path.to_str().ok_or_else(|| err("BACKUP_PATH_INVALID"))?;
    if !path.is_absolute()
        || text.len() > 2048
        || text.chars().any(|c| c.is_control() || c == ',')
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(err("BACKUP_PATH_INVALID"));
    }
    if directory {
        let root = crate::windows_private::PrivateDirectory::inspect(path)?;
        root.check()?;
        Ok(root.path().to_owned())
    } else {
        let parent = crate::windows_private::ParentDirectory::inspect(
            path.parent().ok_or_else(|| err("BACKUP_PATH_INVALID"))?,
        )?;
        let canonical = parent
            .path()
            .join(path.file_name().ok_or_else(|| err("BACKUP_PATH_INVALID"))?);
        let file = crate::windows_private::HostFile::open(&canonical)?;
        file.check_private()?;
        if file.metadata()?.len() != 32 {
            return Err(err("BACKUP_PATH_INVALID"));
        }
        parent.check()?;
        Ok(canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_accepts_only_exact_safe_success_and_never_repeats_raw_output() {
        assert_eq!(
            receipt(
                br#"{"operation":"verified","files":4}"#,
                "id",
                "image",
                &"a".repeat(64)
            )
            .unwrap()
            .files,
            4
        );
        for output in [
            br#"{"operation":"created","files":4}"#.as_slice(),
            br#"{"operation":"verified","files":0}"#,
            br#"{"operation":"verified","files":1000001}"#,
            br#"{"operation":"verified","files":4,"secret":"private"}"#,
            b"private connection error",
        ] {
            let error = receipt(output, "id", "image", &"a".repeat(64)).unwrap_err();
            assert_eq!(error.code, "BACKUP_RESULT_INVALID");
            assert!(!serde_json::to_string(&error).unwrap().contains("private"));
        }
    }
    #[test]
    fn reject_untrusted_image_and_overlap_before_engine_operations() {
        let root = std::env::temp_dir().join(format!("manager-verify-test-{}", Uuid::new_v4()));
        let _created = LifecycleService::new(root.clone()).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let service = LifecycleService::new(root.clone()).unwrap();
        assert_eq!(
            service
                .verify_backup("image:latest", &root, &root)
                .unwrap_err()
                .code,
            "BACKUP_IMAGE_INVALID"
        );
        assert!(input_path(Path::new("relative"), true).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            let key = root.join("key");
            fs::write(&key, [0u8; 32]).unwrap();
            fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(
                input_path(&key, false).unwrap_err().code,
                "BACKUP_PRIVATE_PERMISSIONS"
            );
            fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(input_path(&key, false).is_ok());
            let alias = root.join("alias");
            symlink(&key, &alias).unwrap();
            assert!(input_path(&alias, false).is_err());
            assert_eq!(
                service
                    .verify_backup(&format!("sha256:{}", "a".repeat(64)), &key, &root)
                    .unwrap_err()
                    .code,
                "BACKUP_PATH_OVERLAP"
            );
            let _guard = service.lock().unwrap();
            assert_eq!(
                service
                    .verify_backup("image:latest", &key, &root)
                    .unwrap_err()
                    .code,
                "BUSY"
            );
        }
    }
}
