// SPDX-License-Identifier: Apache-2.0
//! Immutable NTFS generation publication. File sync is not a host crash proof.
use super::*;
use fs2::FileExt;

impl PrivateDirectory {
    /// Write a fresh private pending record, sync, and publish without replacement.
    /// Existing names/ACLs are never repaired. Failed candidates remain private.
    /// The returned reader retains native sharing/identity and cannot write bytes.
    pub(crate) fn publish_new_record<'a>(
        &'a self,
        name: &str,
        bytes: &[u8],
        limit: usize,
    ) -> Result<BoundRecord<'a>> {
        valid_name(name)?;
        self.check()?;
        if limit > 16 * 1024 * 1024 || bytes.len() > limit {
            return Err(err("WINDOWS_PROFILE_RECORD_QUOTA"));
        }
        let pending = format!("pending-{}.json", uuid::Uuid::new_v4());
        let mut file =
            self.create_record_access(&pending, fsapi::FILE_SHARE_READ, fsapi::DELETE)?;
        file.try_lock_exclusive()
            .map_err(|_| err("WINDOWS_PROFILE_BUSY"))?;
        self.check_record(&file, &pending)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
        self.check_record(&file, &pending)?;
        let expected = identity(&file, false)?;
        let filename = wide(&self.path.join(name))?;
        if filename.len() > 32768 {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        let size = (mem::offset_of!(fsapi::FILE_RENAME_INFO, FileName) + filename.len() * 2)
            .max(mem::size_of::<fsapi::FILE_RENAME_INFO>());
        let mut buffer = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
        let info = buffer.as_mut_ptr().cast::<fsapi::FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = ptr::null_mut();
            (*info).FileNameLength = ((filename.len() - 1) * 2) as u32;
            ptr::copy_nonoverlapping(
                filename.as_ptr(),
                ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
                filename.len(),
            );
        }
        self.check()?;
        if unsafe {
            fsapi::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                fsapi::FileRenameInfo,
                buffer.as_ptr().cast(),
                size as u32,
            )
        } == 0
        {
            return Err(err("WINDOWS_PROFILE_PUBLICATION_REFUSED"));
        }
        // Visibility may have changed. All later failures are uncertain, never
        // completion or permission to retry/overwrite the generation.
        let finish = (|| {
            file.sync_all()
                .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
            self.check_record(&file, name)?;
            if identity(&file, false)? != expected {
                return Err(err("WINDOWS_PROFILE_IDENTITY_INVALID"));
            }
            drop(file);
            let mut reader = self.read_record(name)?;
            if reader.id != expected || reader.read_bounded(limit)? != bytes {
                return Err(err("WINDOWS_PROFILE_RECORD_CHANGED"));
            }
            reader
                .file
                .seek(SeekFrom::Start(0))
                .map_err(|_| err("WINDOWS_PROFILE_RECORD_IO"))?;
            reader.check()?;
            Ok(reader)
        })();
        finish.map_err(|_| err("WINDOWS_PROFILE_PUBLICATION_UNCERTAIN"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fresh() -> PrivateDirectory {
        let parent = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        PrivateDirectory::create(&parent.join(format!(
            "exhibitos-generation-test-{}",
            uuid::Uuid::new_v4()
        )))
        .unwrap()
    }
    fn cleanup(root: PrivateDirectory) {
        let path = root.path().to_owned();
        root.check().unwrap();
        drop(root);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn windows_generation_publication_reopens_exact_private_bytes_and_retains_reader_fence() {
        let root = fresh();
        let bytes = b"synthetic generation one";
        let mut record = root
            .publish_new_record("00000000000000000001.json", bytes, 1024)
            .unwrap();
        assert_eq!(record.read_bounded(1024).unwrap(), bytes);
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(root.path().join(&record.name))
                .is_err()
        );
        assert!(
            std::fs::rename(
                root.path().join(&record.name),
                root.path().join("moved.json")
            )
            .is_err()
        );
        drop(record);
        assert_eq!(
            root.read_record("00000000000000000001.json")
                .unwrap()
                .read_bounded(1024)
                .unwrap(),
            bytes
        );
        cleanup(root);
    }
    #[test]
    fn windows_generation_publication_existing_and_case_alias_refuse_without_rewrite() {
        let root = fresh();
        drop(
            root.publish_new_record("Generation.json", b"original", 1024)
                .unwrap(),
        );
        assert_eq!(
            root.publish_new_record("generation.JSON", b"replacement", 1024)
                .err()
                .unwrap()
                .code,
            "WINDOWS_PROFILE_PUBLICATION_REFUSED"
        );
        assert_eq!(
            root.read_record("Generation.json")
                .unwrap()
                .read_bounded(1024)
                .unwrap(),
            b"original"
        );
        let pending: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("pending-")
            })
            .collect();
        assert_eq!(pending.len(), 1);
        assert_eq!(std::fs::read(&pending[0]).unwrap(), b"replacement");
        cleanup(root);
    }
    #[test]
    fn windows_generation_publication_invalid_names_and_quota_create_no_candidates() {
        let root = fresh();
        for name in ["../outside.json", "stream:alias", "CON.json", "record. "] {
            assert!(root.publish_new_record(name, b"synthetic", 1024).is_err());
        }
        assert!(
            root.publish_new_record("large.json", b"too big", 1)
                .is_err()
        );
        assert!(
            root.publish_new_record("unbounded.json", b"synthetic", 16 * 1024 * 1024 + 1)
                .is_err()
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        cleanup(root);
    }
    #[test]
    fn windows_generation_publication_two_publishers_never_overwrite_the_winner() {
        let root = fresh();
        let path = root.path().to_owned();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|bytes| {
                let path = path.clone();
                let barrier = barrier.clone();
                let bytes = bytes.to_vec();
                std::thread::spawn(move || {
                    let guard = PrivateDirectory::inspect(&path).unwrap();
                    barrier.wait();
                    let result = guard.publish_new_record("winner.json", &bytes, 1024);
                    (bytes, result.is_ok())
                })
            })
            .collect();
        let outcomes: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|(_, ok)| *ok).count(), 1);
        let winner = &outcomes.iter().find(|(_, ok)| *ok).unwrap().0;
        assert_eq!(
            root.read_record("winner.json")
                .unwrap()
                .read_bounded(1024)
                .unwrap(),
            *winner
        );
        cleanup(root);
    }
}
