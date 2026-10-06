// SPDX-License-Identifier: Apache-2.0
//! Retained diagnostic catalog input; never a native observation or execution permit.
use crate::{Result, err};
use std::path::Path;
#[cfg(unix)]
use std::{
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

pub(super) struct CatalogInput {
    #[cfg(unix)]
    file: File,
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(unix)]
    identity: Metadata,
    #[cfg(unix)]
    parent_identity: Metadata,
    raw: Vec<u8>,
    pin: String,
}
#[cfg(unix)]
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Catalog {
    schema_version: String,
    schema_digest: String,
    migrations: Vec<Migration>,
}
#[cfg(unix)]
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Migration {
    name: String,
    sha256: String,
}
#[cfg(unix)]
fn validate(raw: &[u8], pin: &str, schema: &str) -> Result<()> {
    let refuse = || err("UPDATE_CATALOG_INVALID");
    if raw.is_empty()
        || raw.len() > 65536
        || !crate::hash_valid(pin)
        || !crate::hash_valid(schema)
        || crate::digest(raw) != pin
    {
        return Err(refuse());
    }
    let v: Catalog = serde_json::from_slice(raw).map_err(|_| refuse())?;
    if v.schema_version != "1.0.0-draft.1"
        || !crate::hash_valid(&v.schema_digest)
        || v.migrations.is_empty()
        || v.migrations.len() > 10000
    {
        return Err(refuse());
    }
    let mut previous: Option<&str> = None;
    for row in &v.migrations {
        if row.name.len() > 1024
            || !row.name.as_bytes().first().is_some_and(u8::is_ascii_digit)
            || !row.name.ends_with(".sql")
            || !row
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || !crate::hash_valid(&row.sha256)
            || previous.is_some_and(|p| p >= row.name.as_str())
        {
            return Err(refuse());
        }
        previous = Some(&row.name);
    }
    // Explicit lexical field order stays canonical even when a workspace enables
    // serde_json's preserve_order feature. The schema hash belongs to the signed plan.
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Canonical<'a> {
        migrations: &'a [Migration],
        schema_digest: &'a str,
        schema_version: &'a str,
    }
    let canonical = serde_json::to_vec(&Canonical {
        migrations: &v.migrations,
        schema_digest: &v.schema_digest,
        schema_version: &v.schema_version,
    })
    .map_err(|_| refuse())?;
    if crate::digest(&canonical) != schema {
        return Err(refuse());
    }
    Ok(())
}
#[cfg(unix)]
fn same(a: &Metadata, b: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (
        a.dev(),
        a.ino(),
        a.len(),
        a.mode(),
        a.uid(),
        a.gid(),
        a.nlink(),
        a.mtime(),
        a.mtime_nsec(),
        a.ctime(),
        a.ctime_nsec(),
    ) == (
        b.dev(),
        b.ino(),
        b.len(),
        b.mode(),
        b.uid(),
        b.gid(),
        b.nlink(),
        b.mtime(),
        b.mtime_nsec(),
        b.ctime(),
        b.ctime_nsec(),
    )
}
impl CatalogInput {
    pub(super) fn read(path: &Path, pin: &str, schema: &str) -> Result<Self> {
        #[cfg(not(unix))]
        {
            let _ = (path, pin, schema);
            Err(err("UPDATE_CATALOG_PLATFORM_UNVERIFIED"))
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            if !path.is_absolute() || fs::canonicalize(path).ok().as_deref() != Some(path) {
                return Err(err("UPDATE_CATALOG_INVALID"));
            }
            let parent = path.parent().ok_or_else(|| err("UPDATE_CATALOG_INVALID"))?;
            let pm = fs::symlink_metadata(parent).map_err(|_| err("UPDATE_CATALOG_INVALID"))?;
            if !pm.is_dir() || pm.uid() != unsafe { libc::geteuid() } || pm.mode() & 0o7777 != 0o700
            {
                return Err(err("UPDATE_CATALOG_INVALID"));
            }
            let mut file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .map_err(|_| err("UPDATE_CATALOG_INVALID"))?;
            let identity = file.metadata().map_err(|_| err("UPDATE_CATALOG_INVALID"))?;
            if !identity.is_file()
                || identity.nlink() != 1
                || identity.uid() != unsafe { libc::geteuid() }
                || !matches!(identity.mode() & 0o7777, 0o400 | 0o600)
                || identity.len() == 0
                || identity.len() > 65536
            {
                return Err(err("UPDATE_CATALOG_INVALID"));
            }
            let mut raw = Vec::new();
            Read::by_ref(&mut file)
                .take(65537)
                .read_to_end(&mut raw)
                .map_err(|_| err("UPDATE_CATALOG_INVALID"))?;
            validate(&raw, pin, schema)?;
            let mut input = Self {
                file,
                path: path.to_owned(),
                identity,
                parent_identity: pm,
                raw,
                pin: pin.to_owned(),
            };
            input.recheck()?;
            Ok(input)
        }
    }
    pub(super) fn recheck(&mut self) -> Result<()> {
        #[cfg(not(unix))]
        {
            Err(err("UPDATE_CATALOG_PLATFORM_UNVERIFIED"))
        }
        #[cfg(unix)]
        {
            let refuse = || err("UPDATE_CATALOG_CHANGED");
            if fs::canonicalize(&self.path).ok().as_deref() != Some(self.path.as_path())
                || !same(
                    &self.parent_identity,
                    &fs::symlink_metadata(self.path.parent().ok_or_else(refuse)?)
                        .map_err(|_| refuse())?,
                )
            {
                return Err(refuse());
            }
            if !same(&self.identity, &self.file.metadata().map_err(|_| refuse())?)
                || !same(
                    &self.identity,
                    &fs::symlink_metadata(&self.path).map_err(|_| refuse())?,
                )
            {
                return Err(refuse());
            }
            self.file.seek(SeekFrom::Start(0)).map_err(|_| refuse())?;
            let mut raw = Vec::new();
            Read::by_ref(&mut self.file)
                .take(65537)
                .read_to_end(&mut raw)
                .map_err(|_| refuse())?;
            if raw != self.raw
                || crate::digest(&raw) != self.pin
                || !same(&self.identity, &self.file.metadata().map_err(|_| refuse())?)
                || !same(
                    &self.identity,
                    &fs::symlink_metadata(&self.path).map_err(|_| refuse())?,
                )
            {
                return Err(refuse());
            }
            Ok(())
        }
    }
    pub(super) fn text(&self) -> Result<String> {
        String::from_utf8(self.raw.clone()).map_err(|_| err("UPDATE_CATALOG_INVALID"))
    }
    pub(super) fn pin(&self) -> &str {
        &self.pin
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn value() -> serde_json::Value {
        serde_json::json!({"schemaVersion":"1.0.0-draft.1","schemaDigest":"a".repeat(64),"migrations":[{"name":"001.sql","sha256":"b".repeat(64)},{"name":"002.sql","sha256":"c".repeat(64)}]})
    }
    fn raw() -> Vec<u8> {
        let v = value();
        let sorted: std::collections::BTreeMap<_, _> = v.as_object().unwrap().iter().collect();
        serde_json::to_vec(&sorted).unwrap()
    }
    fn fixture() -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("exhibitos-catalog-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let file = root.join("catalog.json");
        fs::write(&file, raw()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        (root, file)
    }
    #[test]
    fn exact_catalog_schema_pin_and_private_input_reopen() {
        let (root, path) = fixture();
        let mut input =
            CatalogInput::read(&path, &crate::digest(&raw()), &crate::digest(&raw())).unwrap();
        assert_eq!(input.text().unwrap().as_bytes(), raw());
        input.recheck().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn wrong_pin_schema_duplicate_unknown_and_invalid_migrations_refuse() {
        let b = raw();
        assert!(validate(&b, &"d".repeat(64), &crate::digest(&b)).is_err());
        assert!(validate(&b, &crate::digest(&b), &"d".repeat(64)).is_err());
        let b = b"{\"schemaVersion\":\"1.0.0-draft.1\",\"schemaVersion\":\"1.0.0-draft.1\"}";
        assert!(validate(b, &crate::digest(b), &crate::digest(b)).is_err());
        for fault in [
            "unknown",
            "empty",
            "duplicate",
            "unordered",
            "path",
            "command",
            "hash",
        ] {
            let mut v = value();
            match fault {
                "unknown" => v["extra"] = true.into(),
                "empty" => v["migrations"] = serde_json::json!([]),
                "duplicate" => v["migrations"][1] = v["migrations"][0].clone(),
                "unordered" => v["migrations"].as_array_mut().unwrap().reverse(),
                "path" => v["migrations"][1]["name"] = "../002.sql".into(),
                "command" => v["migrations"][1]["name"] = "002\n.sql".into(),
                _ => v["migrations"][1]["sha256"] = "invalid".into(),
            };
            let b = serde_json::to_vec(&v).unwrap();
            assert!(
                validate(&b, &crate::digest(&b), &crate::digest(&b)).is_err(),
                "{fault}"
            );
        }
    }
    #[test]
    fn aliases_hardlinks_public_file_parent_and_oversized_input_refuse() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (root, path) = fixture();
        let pin = crate::digest(&raw());
        let alias = root.join("alias");
        symlink(&path, &alias).unwrap();
        assert!(CatalogInput::read(&alias, &pin, &pin).is_err());
        fs::remove_file(alias).unwrap();
        let linked = root.join("linked");
        fs::hard_link(&path, &linked).unwrap();
        assert!(CatalogInput::read(&path, &pin, &pin).is_err());
        fs::remove_file(linked).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(CatalogInput::read(&path, &pin, &pin).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(CatalogInput::read(&path, &pin, &pin).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(CatalogInput::read(&path, &pin, &pin).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn retained_catalog_refuses_path_replacement_and_permission_change() {
        use std::os::unix::fs::PermissionsExt;
        for replace in [true, false] {
            let (root, path) = fixture();
            let pin = crate::digest(&raw());
            let mut input = CatalogInput::read(&path, &pin, &pin).unwrap();
            if replace {
                fs::rename(&path, root.join("retained")).unwrap();
                fs::write(&path, raw()).unwrap();
            } else {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
            }
            assert!(input.recheck().is_err());
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn retained_catalog_refuses_parent_alias_and_metadata_changes() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        for alias in [true, false] {
            let (root, path) = fixture();
            let pin = crate::digest(&raw());
            let mut input = CatalogInput::read(&path, &pin, &pin).unwrap();
            if alias {
                let moved = root.with_extension("retained");
                fs::rename(&root, &moved).unwrap();
                symlink(&moved, &root).unwrap();
                assert!(input.recheck().is_err());
                fs::remove_file(&root).unwrap();
                fs::remove_dir_all(moved).unwrap();
            } else {
                fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
                assert!(input.recheck().is_err());
                fs::remove_dir_all(root).unwrap();
            }
        }
    }
}
