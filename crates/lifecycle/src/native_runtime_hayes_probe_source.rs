// SPDX-License-Identifier: Apache-2.0
//! Read-only original descriptor for the exact private Hayes Session probe.

use super::{
    err,
    furnishing_lifetime::{FD_MAX, FD_MIN},
    Result,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{FileExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

const SOURCE_BYTES: u64 = 8_383_096;
const SOURCE_SHA256: &str = "e66715a51c9cad0b904d2715d5b27fb80b97d43fe8c2a3cd10276c1b336a2cf6";

#[derive(PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Stamp {
    fn read(metadata: &Metadata, bytes: u64) -> Result<Self> {
        if !metadata.is_file()
            || metadata.len() != bytes
            || metadata.ino() == 0
            || metadata.nlink() != 1
        {
            return Err(invalid());
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes,
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}

pub(super) struct ProbeSourceFd {
    file: File,
    stamp: Stamp,
}

impl ProbeSourceFd {
    pub(super) fn open_original(path: &Path) -> Result<Self> {
        Self::open_exact(path, SOURCE_BYTES, SOURCE_SHA256)
    }

    fn open_exact(path: &Path, bytes: u64, expected_sha256: &str) -> Result<Self> {
        if !path.is_absolute() || fs::canonicalize(path).map_err(|_| invalid())? != path {
            return Err(invalid());
        }
        let opened = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| invalid())?;
        let stamp = Stamp::read(&opened.metadata().map_err(|_| invalid())?, bytes)?;
        if Stamp::read(&fs::symlink_metadata(path).map_err(|_| invalid())?, bytes)? != stamp {
            return Err(invalid());
        }
        let mut buffer = [0u8; 64 * 1024];
        let checked = (|| {
            let mut digest = Sha256::new();
            let mut offset = 0;
            while offset < bytes {
                let size = (bytes - offset).min(buffer.len() as u64) as usize;
                opened
                    .read_exact_at(&mut buffer[..size], offset)
                    .map_err(|_| invalid())?;
                digest.update(&buffer[..size]);
                offset += size as u64;
            }
            if format!("{:x}", digest.finalize()) != expected_sha256
                || Stamp::read(&opened.metadata().map_err(|_| invalid())?, bytes)? != stamp
            {
                return Err(invalid());
            }
            Ok(())
        })();
        buffer.fill(0);
        checked?;

        let fd = unsafe { libc::fcntl(opened.as_raw_fd(), libc::F_DUPFD_CLOEXEC, FD_MIN) };
        if fd < 0 {
            return Err(invalid());
        }
        let source = Self {
            file: unsafe { File::from_raw_fd(fd) },
            stamp,
        };
        source.child_fd()?;
        Ok(source)
    }

    pub(super) fn child_fd(&self) -> Result<i32> {
        let fd = self.file.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        let descriptor_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if !(FD_MIN..=FD_MAX).contains(&fd)
            || flags < 0
            || flags & libc::O_ACCMODE != libc::O_RDONLY
            || descriptor_flags < 0
            || descriptor_flags & libc::FD_CLOEXEC == 0
            || Stamp::read(
                &self.file.metadata().map_err(|_| invalid())?,
                self.stamp.bytes,
            )? != self.stamp
        {
            return Err(invalid());
        }
        Ok(fd)
    }
}

fn invalid() -> crate::LifecycleError {
    err("HAYES_PROBE_SOURCE_INVALID")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_runtime::{
        furnishing_lifetime::WriterLifetime,
        hayes::{operation::HayesSessionPin, tests::Fixture},
        owner_channel::Setup,
    };
    use std::{io::Read, os::unix::fs::symlink, process::Command};
    use uuid::Uuid;

    const BYTES: &[u8] = b"probe source bytes";

    #[test]
    fn source_pin_rejects_wrong_bytes_links_and_later_change() {
        let root = Fixture::new();
        let path = root.path().join("source");
        fs::write(&path, BYTES).unwrap();
        let bytes = BYTES.len() as u64;
        let digest = format!("{:x}", Sha256::digest(BYTES));
        assert!(ProbeSourceFd::open_original(&path).is_err());
        assert!(ProbeSourceFd::open_exact(&path, bytes, &"0".repeat(64)).is_err());
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(ProbeSourceFd::open_exact(&link, bytes, &digest).is_err());
        fs::remove_file(&link).unwrap();
        fs::hard_link(&path, &link).unwrap();
        assert!(ProbeSourceFd::open_exact(&path, bytes, &digest).is_err());
        fs::remove_file(link).unwrap();

        let source = ProbeSourceFd::open_exact(&path, bytes, &digest).unwrap();
        assert!(source.child_fd().is_ok());
        fs::write(&path, vec![b'x'; bytes as usize]).unwrap();
        assert!(source.child_fd().is_err());
    }

    #[test]
    fn only_selected_setup_inherits_the_exact_readonly_probe_fd() {
        let root = Fixture::new();
        let path = root.path().join("source");
        fs::write(&path, BYTES).unwrap();
        let digest = format!("{:x}", Sha256::digest(BYTES));
        let open = || ProbeSourceFd::open_exact(&path, BYTES.len() as u64, &digest).unwrap();

        let ordinary = Setup::new().unwrap();
        let mut command = Command::new("/usr/bin/true");
        command.env_clear();
        ordinary.configure(&mut command).unwrap();
        assert!(!command
            .get_envs()
            .any(|(name, _)| name == "EXHIBITOS_TEST_HAYES_SOURCE_FD"));
        assert!(ordinary.with_hayes_probe_source(open()).is_err());

        let writer = WriterLifetime::acquire(root.path()).unwrap();
        let mut setup = Setup::new()
            .unwrap()
            .with_furnishing_writer(writer.duplicate_for_writer().unwrap())
            .with_hayes(HayesSessionPin::new(Uuid::new_v4(), &"a".repeat(64)).unwrap())
            .unwrap()
            .with_hayes_probe_source(open())
            .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.env_clear().args([
            "--ignored",
            "--exact",
            "native_runtime::hayes_probe_source::tests::inherited_probe_source_child",
        ]);
        setup.configure(&mut child).unwrap();
        assert!(!child
            .get_envs()
            .any(|(_, value)| value == Some(path.as_os_str())));
        assert!(!child.get_args().any(|value| value == path.as_os_str()));
        let mut child = child.spawn().unwrap();
        setup.close_child();
        assert!(child.wait().unwrap().success());
    }

    #[test]
    #[ignore = "spawned only by the selected descriptor inheritance test"]
    fn inherited_probe_source_child() {
        let vars = [
            "EXHIBITOS_TEST_HAYES_SOURCE_FD",
            "EXHIBITOS_NATIVE_OWNER_FD",
            "EXHIBITOS_NATIVE_HAYES_OWNER_FD",
            "EXHIBITOS_NATIVE_FURNISHING_WRITER_FD",
        ];
        let [source, owner, hayes, writer] = vars.map(|key| {
            let fd: i32 = std::env::var(key).unwrap().parse().unwrap();
            assert!((FD_MIN..=FD_MAX).contains(&fd));
            assert!(unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0);
            fd
        });
        let fds = [source, owner, hayes, writer];
        for (i, fd) in fds.iter().enumerate() {
            assert!(!fds[..i].contains(fd));
        }
        let flags = unsafe { libc::fcntl(source, libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
        let mut source = unsafe { File::from_raw_fd(source) };
        assert!(source.metadata().unwrap().is_file());
        let mut actual = Vec::new();
        source.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, BYTES);
    }
}
