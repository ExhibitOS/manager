// SPDX-License-Identifier: Apache-2.0
//! The lock outlives Manager if a writer or validator retains its inherited FD.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

const NAME: &str = "native-workspace-furnishing-writers.lock";

pub(super) struct WriterLifetime {
    file: File,
    path: PathBuf,
}

impl WriterLifetime {
    pub(super) fn acquire(root: &Path) -> Result<Self> {
        let path = root.join(NAME);
        let file = OpenOptions::new().read(true).write(true).create(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&path).map_err(|_| err("NATIVE_FURNISHING_WRITER_LOCK_INVALID"))?;
        let lifetime = Self { file, path };
        lifetime.check()?;
        lifetime.file.try_lock_exclusive().map_err(|error| {
            err(if error.kind() == std::io::ErrorKind::WouldBlock {
                "NATIVE_FURNISHING_WRITER_BUSY"
            } else {
                "NATIVE_FURNISHING_WRITER_LOCK_INVALID"
            })
        })?;
        lifetime.check()?;
        // Unlike OperationGuard, dropping this type only closes the file.
        Ok(lifetime)
    }

    fn check(&self) -> Result<()> {
        let invalid = || err("NATIVE_FURNISHING_WRITER_LOCK_INVALID");
        let held = self.file.metadata().map_err(|_| invalid())?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| invalid())?;
        if !held.is_file() || held.uid() != unsafe { libc::geteuid() }
            || held.mode() & 0o777 != 0o600 || held.nlink() != 1
            || (held.dev(), held.ino()) != (named.dev(), named.ino())
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn duplicate_for_writer(&self) -> Result<File> {
        self.check()?;
        let fd = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        if fd < 0 { return Err(err("NATIVE_FURNISHING_WRITER_LOCK_INVALID")); }
        let file = unsafe { File::from_raw_fd(fd) };
        if fd > 1023 { return Err(err("NATIVE_FURNISHING_WRITER_LOCK_INVALID")); }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

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
