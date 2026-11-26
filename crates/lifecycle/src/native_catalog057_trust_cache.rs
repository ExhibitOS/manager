// SPDX-License-Identifier: Apache-2.0
//! Metadata-only preservation of the exact reviewed Catalog057 trust cache.
use super::*;
use std::{
    io::{Read, Seek},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};

const CACHE: &str = "default-catalogue-trust";
const PRESERVED_ROOT: &str = "tuf-repo.github.com/root.json";

struct Entry {
    path: &'static str,
    children: &'static [&'static str],
    bytes: u64,
}

const ENTRIES: [Entry; 8] = [
    Entry {
        path: "",
        children: &["tuf-repo.github.com"],
        bytes: 0,
    },
    Entry {
        path: "tuf-repo.github.com",
        children: &[
            "root.json",
            "snapshot.json",
            "targets",
            "targets.json",
            "timestamp.json",
        ],
        bytes: 0,
    },
    Entry {
        path: "tuf-repo.github.com/targets",
        children: &["trusted_root.json"],
        bytes: 0,
    },
    Entry {
        path: PRESERVED_ROOT,
        children: &[],
        bytes: 6734,
    },
    Entry {
        path: "tuf-repo.github.com/snapshot.json",
        children: &[],
        bytes: 446,
    },
    Entry {
        path: "tuf-repo.github.com/targets.json",
        children: &[],
        bytes: 2974,
    },
    Entry {
        path: "tuf-repo.github.com/timestamp.json",
        children: &[],
        bytes: 450,
    },
    Entry {
        path: "tuf-repo.github.com/targets/trusted_root.json",
        children: &[],
        bytes: 31645,
    },
];

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Proof {
    entries: [EntryPin; ENTRIES.len()],
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EntryPin {
    path: String,
    inode: u64,
    sha256: Option<String>,
}

struct Held<'a> {
    path: PathBuf,
    entry: &'a Entry,
    pin: &'a EntryPin,
    file: File,
    device: u64,
}

fn invalid() -> LifecycleError {
    err("NATIVE_CATALOGUE_TRUST_CACHE_UNCONFIRMED")
}

impl Held<'_> {
    fn check_name(&self) -> Result<u32> {
        let held = self.file.metadata().map_err(|_| invalid())?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| invalid())?;
        let expected = (self.device, self.pin.inode);
        let directory = !self.entry.children.is_empty();
        let (before, after) = self.modes();
        let mode = held.mode() & 0o7777;
        if (held.dev(), held.ino()) != expected
            || (named.dev(), named.ino()) != expected
            || held.uid() != unsafe { libc::geteuid() }
            || held.mode() != named.mode()
            || (mode != before && mode != after)
            || fs::canonicalize(&self.path).ok().as_deref() != Some(self.path.as_path())
            || if directory {
                !held.is_dir()
            } else {
                !held.is_file() || held.nlink() != 1 || held.len() != self.entry.bytes
            }
        {
            return Err(invalid());
        }
        Ok(mode)
    }

    fn modes(&self) -> (u32, u32) {
        if self.entry.path == PRESERVED_ROOT {
            (0o600, 0o600)
        } else if self.entry.children.is_empty() {
            (0o644, 0o600)
        } else {
            (0o755, 0o700)
        }
    }

    fn verify(&mut self) -> Result<u32> {
        let mode = self.check_name()?;
        if self.entry.children.is_empty() {
            self.file.rewind().map_err(|_| invalid())?;
            let mut bytes = Vec::new();
            (&mut self.file)
                .take(self.entry.bytes + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| invalid())?;
            if bytes.len() as u64 != self.entry.bytes
                || self.pin.sha256.as_deref() != Some(digest(&bytes).as_str())
            {
                return Err(invalid());
            }
        } else {
            // Read one extra entry to detect an expanded tree without traversing it.
            let mut names = fs::read_dir(&self.path)
                .map_err(|_| invalid())?
                .take(self.entry.children.len() + 1)
                .map(|item| item.map(|entry| entry.file_name()).map_err(|_| invalid()))
                .collect::<Result<Vec<_>>>()?;
            names.sort();
            if names != self.entry.children || self.pin.sha256.is_some() {
                return Err(invalid());
            }
        }
        if self.check_name()? != mode {
            return Err(invalid());
        }
        Ok(mode)
    }
}

pub(super) fn privatize(root: &Path, device: u64, proof: &Proof) -> Result<usize> {
    let config = root.join("config");
    private_directory(&config)?;
    let parent = DirectoryPin::open(&config)?;
    if parent
        .try_clone_file()?
        .metadata()
        .map_err(|_| invalid())?
        .dev()
        != device
    {
        return Err(invalid());
    }

    let mut held = Vec::with_capacity(ENTRIES.len());
    for (entry, pin) in ENTRIES.iter().zip(&proof.entries) {
        if pin.path != entry.path {
            return Err(invalid());
        }
        let path = config.join(CACHE).join(entry.path);
        let flags = libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(if entry.children.is_empty() {
                flags
            } else {
                flags | libc::O_DIRECTORY
            })
            .open(&path)
            .map_err(|_| invalid())?;
        let mut member = Held {
            path,
            entry,
            pin,
            file,
            device,
        };
        member.verify()?;
        held.push(member);
    }

    // All pins must pass before the first change. Tighten parent directories
    // first. An interruption can leave only old or private modes, so the same
    // reviewed proof can resume without reverting completed changes.
    parent.check()?;
    let mut changed = 0;
    for member in &mut held {
        let private = member.modes().1;
        if member.verify()? != private {
            member
                .file
                .set_permissions(fs::Permissions::from_mode(private))
                .map_err(|_| invalid())?;
            changed += 1;
        }
        if member.entry.path != PRESERVED_ROOT {
            member.file.sync_all().map_err(|_| invalid())?;
        }
    }
    parent.check()?;
    for member in &mut held {
        if member.verify()? != member.modes().1 {
            return Err(invalid());
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, symlink};

    struct Fixture {
        root: PathBuf,
        proof: Proof,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(Uuid::new_v4().to_string());
            let private = |path: &Path| fs::DirBuilder::new().mode(0o700).create(path).unwrap();
            private(&root);
            private(&root.join("config"));
            let cache = root.join("config").join(CACHE);
            let entries = ENTRIES.each_ref().map(|entry| {
                let path = cache.join(entry.path);
                let (sha256, mode) = if entry.children.is_empty() {
                    let bytes = vec![42; entry.bytes as usize];
                    fs::write(&path, &bytes).unwrap();
                    let mode = if entry.path == PRESERVED_ROOT {
                        0o600
                    } else {
                        0o644
                    };
                    (Some(digest(&bytes)), mode)
                } else {
                    private(&path);
                    (None, 0o755)
                };
                fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
                EntryPin {
                    path: entry.path.into(),
                    inode: path.metadata().unwrap().ino(),
                    sha256,
                }
            });
            Self {
                root,
                proof: Proof { entries },
            }
        }

        fn path(&self, index: usize) -> PathBuf {
            self.root
                .join("config")
                .join(CACHE)
                .join(ENTRIES[index].path)
        }

        fn repair(&self, proof: &Proof) -> Result<usize> {
            privatize(&self.root, self.root.metadata().unwrap().dev(), proof)
        }

        fn mode(&self, index: usize) -> u32 {
            self.path(index).symlink_metadata().unwrap().mode() & 0o7777
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn exact_catalog057_trust_cache_tightens_modes_and_resumes_same_proof() {
        let fresh = Fixture::new();
        let root_before = fresh.path(3).metadata().unwrap();
        assert_eq!(fresh.repair(&fresh.proof).unwrap(), 7);
        let root_after = fresh.path(3).metadata().unwrap();
        assert_eq!(root_after.ino(), root_before.ino());
        assert_eq!(root_after.mode(), root_before.mode());
        assert_eq!(
            (root_after.ctime(), root_after.ctime_nsec()),
            (root_before.ctime(), root_before.ctime_nsec())
        );
        let fixture = Fixture::new();
        fs::set_permissions(fixture.path(0), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(fixture.path(4), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fixture.repair(&fixture.proof).unwrap(), 5);
        assert_eq!(fixture.repair(&fixture.proof).unwrap(), 0);
        for (index, pin) in fixture.proof.entries.iter().enumerate() {
            let path = fixture.path(index);
            assert_eq!(path.metadata().unwrap().ino(), pin.inode);
            if let Some(sha) = &pin.sha256 {
                assert_eq!(fixture.mode(index), 0o600);
                assert_eq!(&digest(&fs::read(path).unwrap()), sha);
            } else {
                assert_eq!(fixture.mode(index), 0o700);
            }
        }
    }

    #[test]
    fn exact_catalog057_trust_cache_rejects_changed_pins_modes_names_and_links_before_writes() {
        let fixture = Fixture::new();
        let mut wrong = fixture.proof.clone();
        wrong.entries[6].inode += 1;
        assert!(fixture.repair(&wrong).is_err());
        assert!(
            privatize(
                &fixture.root,
                fixture.root.metadata().unwrap().dev() + 1,
                &fixture.proof
            )
            .is_err()
        );
        let mut wrong = fixture.proof.clone();
        wrong.entries[6].sha256 = Some("0".repeat(64));
        assert!(fixture.repair(&wrong).is_err());
        let mut wrong = fixture.proof.clone();
        wrong.entries.swap(3, 4);
        assert!(fixture.repair(&wrong).is_err());

        let root = fixture.path(3);
        let mut wrong_root = fixture.proof.clone();
        wrong_root.entries[3].sha256 = Some("0".repeat(64));
        assert!(fixture.repair(&wrong_root).is_err());
        wrong_root = fixture.proof.clone();
        wrong_root.entries[3].inode += 1;
        assert!(fixture.repair(&wrong_root).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(fixture.repair(&fixture.proof).is_err());
        assert_eq!(fixture.mode(3), 0o644);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o600)).unwrap();

        let last = fixture.path(7);
        fs::set_permissions(&last, fs::Permissions::from_mode(0o664)).unwrap();
        assert!(fixture.repair(&fixture.proof).is_err());
        fs::set_permissions(&last, fs::Permissions::from_mode(0o644)).unwrap();
        let extra = fixture.path(0).join("extra");
        fs::write(&extra, b"extra").unwrap();
        assert!(fixture.repair(&fixture.proof).is_err());
        fs::remove_file(&extra).unwrap();
        let alias = fixture.root.join("alias");
        fs::hard_link(&last, &alias).unwrap();
        assert!(fixture.repair(&fixture.proof).is_err());
        fs::remove_file(&alias).unwrap();
        fs::rename(&last, &alias).unwrap();
        symlink(&alias, &last).unwrap();
        assert!(fixture.repair(&fixture.proof).is_err());
        assert_eq!(fixture.mode(0), 0o755);
        assert_eq!(alias.metadata().unwrap().mode() & 0o7777, 0o644);
    }
}
