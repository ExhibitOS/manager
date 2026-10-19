// SPDX-License-Identifier: Apache-2.0
//! Offline native locators, never trust provisioning or an execution permit.
use super::*;
use crate::signed_release::{Release, VerifiedRelease};
use std::io::{Seek, SeekFrom};
use std::os::unix::{
    fs::{MetadataExt, OpenOptionsExt},
    io::{AsRawFd, FromRawFd},
};
const MAX_FILES: usize = 64;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Inputs {
    pub folder: PathBuf,
    pub envelope_name: String,
    pub envelope_sha256: String,
    pub artifact_name: String,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub catalog: Option<(String, String)>,
    pub key_file: PathBuf,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub available: bool,
    pub code: Option<String>,
    pub version: Option<String>,
    pub changed_schema: Option<bool>,
}
fn invalid() -> LifecycleError {
    err("UPDATE_PREPARATION_INPUT_INVALID")
}
/// Mirrors the existing OCI qualifier admission, which remains authoritative.
pub(crate) fn supported_release(release: &Release) -> Result<()> {
    if release.channel != "development"
        || !matches!(release.target.as_str(), "linux-arm64" | "linux-amd64")
    {
        return Err(err("UPDATE_OCI_PLATFORM_UNVERIFIED"));
    }
    Ok(())
}
fn identity(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    (
        a.dev(),
        a.ino(),
        a.mode(),
        a.uid(),
        a.gid(),
        a.nlink(),
        a.len(),
        a.mtime(),
        a.mtime_nsec(),
        a.ctime(),
        a.ctime_nsec(),
    ) == (
        b.dev(),
        b.ino(),
        b.mode(),
        b.uid(),
        b.gid(),
        b.nlink(),
        b.len(),
        b.mtime(),
        b.mtime_nsec(),
        b.ctime(),
        b.ctime_nsec(),
    )
}
pub(crate) struct Directory {
    path: PathBuf,
    file: File,
    identity: fs::Metadata,
}
impl Directory {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || path
                .to_str()
                .is_none_or(|p| p.len() > 2048 || p.chars().any(|c| c.is_control() || c == ','))
            || fs::canonicalize(path).ok().as_deref() != Some(path)
        {
            return Err(invalid());
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| invalid())?;
        let metadata = file.metadata().map_err(|_| invalid())?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            return Err(invalid());
        }
        let v = Self {
            path: path.into(),
            file,
            identity: metadata,
        };
        v.check()?;
        Ok(v)
    }
    pub(crate) fn check(&self) -> Result<()> {
        let a = fs::symlink_metadata(&self.path).map_err(|_| invalid())?;
        let b = self.file.metadata().map_err(|_| invalid())?;
        let pin = |m: &fs::Metadata| (m.dev(), m.ino(), m.uid(), m.gid(), m.mode());
        if !a.is_dir()
            || a.is_symlink()
            || fs::canonicalize(&self.path).ok().as_deref() != Some(&self.path)
            || pin(&a) != pin(&self.identity)
            || pin(&b) != pin(&self.identity)
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn file(&self, name: &str, limit: u64, private: bool) -> Result<InputFile> {
        if name.is_empty()
            || name.len() > 255
            || name == "."
            || name == ".."
            || name.contains(['/', '\\'])
        {
            return Err(invalid());
        }
        self.check()?;
        let text = std::ffi::CString::new(name).map_err(|_| invalid())?;
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                text.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(invalid());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let m = file.metadata().map_err(|_| invalid())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || m.mode() & 0o022 != 0
            || private && m.mode() & 0o7777 != 0o600
            || m.len() == 0
            || m.len() > limit
        {
            return Err(invalid());
        }
        let v = InputFile {
            file,
            path: self.path.join(name),
            identity: m,
        };
        v.check()?;
        self.check()?;
        Ok(v)
    }
}
pub(crate) struct InputFile {
    pub file: File,
    path: PathBuf,
    identity: fs::Metadata,
}
impl InputFile {
    pub(crate) fn check(&self) -> Result<()> {
        let m = fs::symlink_metadata(&self.path).map_err(|_| invalid())?;
        if m.is_symlink()
            || !identity(&self.identity, &m)
            || !identity(
                &self.identity,
                &self.file.metadata().map_err(|_| invalid())?,
            )
            || fs::canonicalize(&self.path).ok().as_deref() != Some(&self.path)
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn read(&mut self, limit: u64) -> Result<Vec<u8>> {
        self.check()?;
        self.file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let mut b = Vec::new();
        Read::by_ref(&mut self.file)
            .take(limit + 1)
            .read_to_end(&mut b)
            .map_err(|_| invalid())?;
        if b.len() as u64 > limit {
            return Err(invalid());
        }
        self.check()?;
        Ok(b)
    }
    pub(crate) fn pin(&mut self) -> Result<String> {
        self.check()?;
        self.file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        let mut h = Sha256::new();
        let mut count = 0;
        let mut b = [0; 65536];
        loop {
            let n = self.file.read(&mut b).map_err(|_| invalid())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > self.identity.len() {
                return Err(invalid());
            }
            h.update(&b[..n]);
        }
        self.check()?;
        if count != self.identity.len() {
            return Err(invalid());
        }
        self.file.seek(SeekFrom::Start(0)).map_err(|_| invalid())?;
        Ok(format!("{:x}", h.finalize()))
    }
    pub(crate) fn len(&self) -> u64 {
        self.identity.len()
    }
}
pub(crate) fn key(path: &Path, profile: &Path, source: &Path) -> Result<InputFile> {
    maintenance::input_path(path, false)?;
    if path.starts_with(profile) || path.starts_with(source) {
        return Err(err("BACKUP_PATH_OVERLAP"));
    }
    let parent = Directory::open(path.parent().ok_or_else(invalid)?)?;
    let v = parent.file(
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(invalid)?,
        32,
        true,
    )?;
    if v.len() != 32 {
        return Err(invalid());
    }
    Ok(v)
}
/// Verify using the independent native Store. Discovery never imports policy.
pub(crate) fn discover(
    folder: &Path,
    key_file: &Path,
    source_schema: &str,
    scratch: &Path,
    mut verify: impl FnMut(&[u8]) -> Result<VerifiedRelease>,
) -> Result<(Inputs, Release)> {
    let directory = Directory::open(folder)?;
    let mut names = Vec::new();
    let mut count = 0;
    for item in fs::read_dir(folder)
        .map_err(|_| invalid())?
        .take(MAX_FILES + 1)
    {
        count += 1;
        if count > MAX_FILES {
            return Err(invalid());
        }
        let name = item
            .map_err(|_| invalid())?
            .file_name()
            .into_string()
            .map_err(|_| invalid())?;
        if name.ends_with(".json") {
            names.push(name);
        }
    }
    names.sort();
    let mut found = None;
    let mut refusal = None;
    for name in &names {
        let mut input =
            match directory.file(name, crate::signed_release::MAX_ENVELOPE as u64, false) {
                Ok(v) => v,
                Err(_) => continue,
            };
        let raw = input.read(crate::signed_release::MAX_ENVELOPE as u64)?;
        if serde_json::from_slice::<crate::signed_release::Envelope>(&raw).is_err() {
            continue;
        }
        match verify(&raw) {
            Ok(v) => {
                if found.is_some() {
                    return Err(err("UPDATE_PREPARATION_AMBIGUOUS"));
                }
                found = Some((name.clone(), raw, v));
            }
            Err(e) => refusal = Some(e),
        }
    }
    let (envelope_name, raw, mut verified) =
        found.ok_or_else(|| refusal.unwrap_or_else(|| err("UPDATE_ENVELOPE_INVALID")))?;
    let release = verified.release().clone();
    supported_release(&release)?;
    let mut artifact = directory.file(&release.artifact.name, 2 * 1024 * 1024 * 1024, false)?;
    if artifact.len() != release.artifact.bytes {
        return Err(err("UPDATE_ARTIFACT_MISMATCH"));
    }
    verified
        .verify_artifact(&mut artifact.file)
        .map_err(|e| err(e.code()))?;
    artifact.check()?;
    let mut catalog = None;
    if release.artifact.schema_sha256 != source_schema {
        for name in &names {
            let mut file = match directory.file(name, 65536, false) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let raw = file.read(65536)?;
            let pin = digest(&raw);
            let private = scratch.join(format!("{}.json", Uuid::new_v4()));
            write_private_new(
                scratch,
                private
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(invalid)?,
                &raw,
                65536,
            )?;
            if crate::signed_release::trust::validate_preparation_catalog(
                &private,
                &pin,
                &release.artifact.schema_sha256,
            )
            .is_ok()
            {
                if catalog.is_some() {
                    return Err(err("UPDATE_PREPARATION_AMBIGUOUS"));
                }
                catalog = Some((name.clone(), pin));
            }
        }
        if catalog.is_none() {
            return Err(err("UPDATE_CATALOG_REQUIRED"));
        }
    }
    directory.check()?;
    Ok((
        Inputs {
            folder: folder.into(),
            envelope_name,
            envelope_sha256: digest(&raw),
            artifact_name: release.artifact.name.clone(),
            artifact_sha256: release.artifact.sha256.clone(),
            artifact_bytes: release.artifact.bytes,
            catalog,
            key_file: key_file.into(),
        },
        release,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn directory() -> PathBuf {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-preparation-input-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        root
    }
    #[test]
    fn retained_source_change_is_rejected_without_modifying_original() {
        let root = directory();
        fs::write(root.join("artifact"), b"original").unwrap();
        fs::set_permissions(root.join("artifact"), fs::Permissions::from_mode(0o600)).unwrap();
        let parent = Directory::open(&root).unwrap();
        let mut input = parent.file("artifact", 1024, false).unwrap();
        assert_eq!(input.read(1024).unwrap(), b"original");
        fs::write(root.join("artifact"), b"changed").unwrap();
        assert!(input.read(1024).is_err());
        assert_eq!(fs::read(root.join("artifact")).unwrap(), b"changed");
    }
    #[test]
    fn source_symlink_and_key_overlap_do_not_gain_admission() {
        let root = directory();
        fs::write(root.join("real"), [7; 32]).unwrap();
        fs::set_permissions(root.join("real"), fs::Permissions::from_mode(0o600)).unwrap();
        symlink(root.join("real"), root.join("alias")).unwrap();
        let dir = Directory::open(&root).unwrap();
        assert!(dir.file("alias", 32, true).is_err());
        assert!(key(&root.join("real"), &root, &root.join("runtime")).is_err());
        assert_eq!(fs::read(root.join("real")).unwrap(), [7; 32]);
    }
}
