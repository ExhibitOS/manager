// SPDX-License-Identifier: Apache-2.0
//! The lock outlives Manager if a writer or validator retains its inherited FD.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

const NAME: &str = "native-workspace-furnishing-writers.lock";
pub(super) const FD_MIN: i32 = 10;
pub(super) const FD_MAX: i32 = 1023;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct WriterIdentity {
    pub(super) root_device: u64,
    pub(super) root_inode: u64,
    pub(super) lock_device: u64,
    pub(super) lock_inode: u64,
}

pub(super) struct ProbedWriter {
    file: File,
    pub(super) identity: WriterIdentity,
}

impl AsRawFd for ProbedWriter {
    fn as_raw_fd(&self) -> i32 { self.file.as_raw_fd() }
}

fn invalid() -> LifecycleError { err("NATIVE_FURNISHING_WRITER_LOCK_INVALID") }

pub(super) struct WriterLifetime {
    file: File,
    path: PathBuf,
    root: crate::maintenance_source::DirectoryPin,
}

impl WriterLifetime {
    pub(super) fn acquire(root: &Path) -> Result<Self> {
        let path = root.join(NAME);
        let root = crate::maintenance_source::DirectoryPin::open(root).map_err(|_| invalid())?;
        let file = OpenOptions::new().read(true).write(true).create(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&path).map_err(|_| err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"))?;
        let lifetime = Self { file, path, root };
        lifetime.identity()?;
        lifetime.file.try_lock_exclusive().map_err(|error| {
            err(if error.kind() == std::io::ErrorKind::WouldBlock {
                "NATIVE_FURNISHING_WRITER_BUSY"
            } else {
                "NATIVE_FURNISHING_WRITER_LOCK_INVALID"
            })
        })?;
        lifetime.identity()?;
        // Unlike OperationGuard, dropping this type only closes the file.
        Ok(lifetime)
    }

    fn check_file(file: &File, device: u64, inode: u64) -> Result<()> {
        let held = file.metadata().map_err(|_| invalid())?;
        if !held.is_file() || held.uid() != unsafe { libc::geteuid() }
            || held.mode() & 0o777 != 0o600 || held.nlink() != 1
            || (held.dev(), held.ino()) != (device, inode)
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn identity(&self) -> Result<WriterIdentity> {
        self.root.check_path(self.path.parent().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let root = self.root.try_clone_file().and_then(|file| file.metadata().map_err(|_| invalid()))?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| invalid())?;
        Self::check_file(&self.file, named.dev(), named.ino())?;
        Ok(WriterIdentity { root_device: root.dev(), root_inode: root.ino(), lock_device: named.dev(), lock_inode: named.ino() })
    }

    pub(super) fn duplicate_for_writer(&self) -> Result<ProbedWriter> {
        let identity = self.identity()?;
        let fd = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, FD_MIN) };
        if fd < 0 { return Err(invalid()); }
        let file = unsafe { File::from_raw_fd(fd) };
        if fd > FD_MAX { return Err(invalid()); }
        self.probe(&file, identity)?;
        Ok(ProbedWriter { file, identity })
    }

    fn probe(&self, inherited: &File, identity: WriterIdentity) -> Result<()> {
        Self::check_file(inherited, identity.lock_device, identity.lock_inode)?;
        let independent = OpenOptions::new().read(true).write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&self.path).map_err(|_| invalid())?;
        Self::check_file(&independent, identity.lock_device, identity.lock_inode)?;
        let blocked = || matches!(independent.try_lock_exclusive(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock);
        // The original stays locked while we probe its exact duplicate. A
        // separate descriptor for the same inode cannot pass both checks.
        if !blocked() || inherited.try_lock_exclusive().is_err() || !blocked() || self.identity()? != identity {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn furnishing_probe_rejects_separate_open_and_replaced_inode() {
        let path = std::env::temp_dir().join(format!("exhibitos-writer-probe-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let root = fs::canonicalize(path).unwrap();
        let lifetime = WriterLifetime::acquire(&root).unwrap();
        let intended = lifetime.duplicate_for_writer().unwrap();
        let separate = OpenOptions::new().read(true).write(true).open(root.join(NAME)).unwrap();
        assert!(lifetime.probe(&separate, intended.identity).is_err());
        fs::rename(root.join(NAME), root.join("prior-lock")).unwrap();
        let replacement = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600)
            .open(root.join(NAME)).unwrap();
        assert!(lifetime.probe(&replacement, intended.identity).is_err());
        assert!(lifetime.duplicate_for_writer().is_err());
        drop((replacement, separate, intended, lifetime));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn close_only_lifetime_survives_writer_exec_and_excludes_unrelated_exec() {
        let path = std::env::temp_dir().join(format!("exhibitos-writer-lifetime-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let root = fs::canonicalize(path).unwrap();
        let original = WriterLifetime::acquire(&root).unwrap();
        let inherited = original.duplicate_for_writer().unwrap();
        let mut setup = owner_channel::Setup::new().unwrap().with_furnishing_writer(inherited);

        let mut unrelated = Command::new("/bin/cat")
            .stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
        let mut writer = Command::new("/bin/cat");
        writer.stdin(Stdio::piped()).stdout(Stdio::null());
        setup.configure(&mut writer).unwrap();
        let mut writer = writer.spawn().unwrap();
        setup.close_child();
        drop(setup);
        drop(original);
        let blocked = WriterLifetime::acquire(&root).err().map(|e| e.code);
        assert!(writer.wait().unwrap().success());
        let next = WriterLifetime::acquire(&root);
        assert!(unrelated.wait().unwrap().success());
        assert_eq!(blocked.as_deref(), Some("NATIVE_FURNISHING_WRITER_BUSY"));
        drop(next.unwrap());
        fs::remove_dir_all(&root).unwrap();
    }
}
