// SPDX-License-Identifier: Apache-2.0
//! Native-only reuse of the selected local runtime's existing CMS credentials.
use super::{LifecycleError, LifecycleService, err};
use std::{fs, io::Read, path::Path};
pub struct LocalPublicationCredentials {
    pub origin: String,
    pub tenant_id: String,
    pub subject: String,
    password: Vec<u8>,
}
impl LocalPublicationCredentials {
    pub fn password(&self) -> &[u8] {
        &self.password
    }
}
impl Drop for LocalPublicationCredentials {
    fn drop(&mut self) {
        self.password.fill(0);
    }
}
struct PrivateBytes(Vec<u8>);
impl Drop for PrivateBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
pub(crate) fn parse(bytes: &[u8], origin: String) -> Result<LocalPublicationCredentials, LifecycleError> {
    let text = std::str::from_utf8(bytes).map_err(|_| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if ["TENANT_ID", "ADMIN_SUBJECT", "ADMIN_PASSWORD"].contains(&key)
            && values.insert(key, value).is_some()
        {
            return Err(err("LOCAL_CMS_CREDENTIALS_INVALID"));
        }
    }
    let tenant = values
        .get("TENANT_ID")
        .ok_or_else(|| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let subject = values
        .get("ADMIN_SUBJECT")
        .ok_or_else(|| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let encoded_password = values
        .get("ADMIN_PASSWORD")
        .ok_or_else(|| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let password=super::initial_account::decode_password(encoded_password).map_err(|_|err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    if uuid::Uuid::parse_str(tenant).is_err()
        || !(3..=128).contains(&subject.len())
        || !subject
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && b"_.@-".contains(&b)))
        || !(12..=1024).contains(&password.len())
        || password.bytes().any(|b| b < 32 || b == 127)
    {
        return Err(err("LOCAL_CMS_CREDENTIALS_INVALID"));
    }
    Ok(LocalPublicationCredentials {
        origin,
        tenant_id: tenant.to_string(),
        subject: subject.to_string(),
        password: password.as_bytes().to_vec(),
    })
}
#[cfg(unix)]
pub(super) fn read(root: &Path, origin: String) -> Result<LocalPublicationCredentials, LifecycleError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let path = root.join("runtime.env");
    let before = fs::symlink_metadata(&path).map_err(|_| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let safe = |m: &fs::Metadata| {
        m.is_file()
            && !m.file_type().is_symlink()
            && m.len() <= 8192
            && m.mode() & 0o777 == 0o600
            && m.nlink() == 1
            && m.uid() == unsafe { libc::geteuid() }
    };
    if !safe(&before) {
        return Err(err("LOCAL_CMS_CREDENTIALS_INVALID"));
    }
    let same = |a: &fs::Metadata, b: &fs::Metadata| {
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    };
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let opened = file
        .metadata()
        .map_err(|_| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    if !safe(&opened) || !same(&before, &opened) {
        return Err(err("LOCAL_CMS_CREDENTIALS_INVALID"));
    }
    let mut bytes = PrivateBytes(Vec::new());
    file.by_ref()
        .take(8193)
        .read_to_end(&mut bytes.0)
        .map_err(|_| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let after = fs::symlink_metadata(&path).map_err(|_| err("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    if bytes.0.len() > 8192 || !safe(&after) || !same(&opened, &after) {
        return Err(err("LOCAL_CMS_CREDENTIALS_INVALID"));
    }
    parse(&bytes.0, origin)
}
impl LifecycleService {
    pub fn local_publication_output(&self) -> Result<std::path::PathBuf, LifecycleError> {
        self.open_url()?;
        let parent = self.root.join("publication-exports");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt};
            match fs::DirBuilder::new().mode(0o700).create(&parent) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(_) => return Err(err("LOCAL_CMS_OUTPUT_UNAVAILABLE")),
            }
            let m =
                fs::symlink_metadata(&parent).map_err(|_| err("LOCAL_CMS_OUTPUT_UNAVAILABLE"))?;
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o777 != 0o700
            {
                return Err(err("LOCAL_CMS_OUTPUT_UNAVAILABLE"));
            }
            Ok(parent.join(format!("publication-{}", uuid::Uuid::new_v4())))
        }
        #[cfg(not(unix))]
        {
            let _ = parent;
            Err(err("LOCAL_CMS_UNSUPPORTED"))
        }
    }
    pub fn local_publication_credentials(
        &self,
    ) -> Result<LocalPublicationCredentials, LifecycleError> {
        let origin = self.open_url()?;
        #[cfg(unix)]
        {
            read(&self.root, origin)
        }
        #[cfg(not(unix))]
        {
            let _ = origin;
            Err(err("LOCAL_CMS_UNSUPPORTED"))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const ENV:&[u8]=b"TENANT_ID=00000000-0000-4000-8000-000000000001\nADMIN_SUBJECT=local-admin\nADMIN_PASSWORD=synthetic-local-password\nDATABASE_URL=must-not-return\n";
    #[test]
    fn parses_only_existing_credentials_and_refuses_duplicate_or_invalid_fields() {
        let c = parse(ENV, "http://127.0.0.1:13200".into()).unwrap();
        assert_eq!(c.subject, "local-admin");
        assert_eq!(c.password(), b"synthetic-local-password");
        assert!(parse(&[ENV, b"ADMIN_SUBJECT=other\n"].concat(), "origin".into()).is_err());
        assert!(
            parse(
                b"TENANT_ID=invalid\nADMIN_SUBJECT=a\nADMIN_PASSWORD=short",
                "origin".into()
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn read_refuses_linked_or_shared_credentials_without_changing_original() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = std::env::temp_dir().join(format!("exhibitos-catalog-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("runtime.env");
        fs::write(&path, ENV).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read(&root, "origin".into()).is_ok());
        let alias = root.join("alias");
        fs::hard_link(&path, &alias).unwrap();
        assert!(read(&root, "origin".into()).is_err());
        fs::remove_file(alias).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read(&root, "origin".into()).is_err());
        fs::remove_file(&path).unwrap();
        symlink("missing", &path).unwrap();
        assert!(read(&root, "origin".into()).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
