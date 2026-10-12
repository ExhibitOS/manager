// SPDX-License-Identifier: Apache-2.0
//! Native private ciphertext transport; Linux helper authentication stays authoritative.
use super::*;
use crate::windows_private::{HostDirectory, HostFile};
use std::collections::BTreeSet;
use std::io::Read;
fn invalid() -> LifecycleError {
    err("BACKUP_RESULT_INVALID")
}
fn name_valid(name: &str) -> bool {
    ["complete.json", "writing.json", "manifest.gcm"].contains(&name)
        || name.strip_prefix("files/").is_some_and(|s| {
            s.len() == 12 && s.ends_with(".gcm") && s.as_bytes()[..8].iter().all(u8::is_ascii_digit)
        })
}
pub(super) fn inventory(result: &HelperResult) -> Result<u64> {
    if result.operation != "created-and-authenticated"
        || Uuid::parse_str(&result.backup_id).is_err()
        || result.files == 0
        || result.files > 10000
        || !hash_valid(&result.authenticated_manifest_sha256)
        || result.archive_inventory.len() != result.files as usize + 3
        || !["complete.json", "writing.json", "manifest.gcm"]
            .iter()
            .all(|n| result.archive_inventory.contains_key(*n))
    {
        return Err(invalid());
    }
    result
        .archive_inventory
        .iter()
        .try_fold(0u64, |sum, (name, item)| {
            if !name_valid(name)
                || item.bytes == 0
                || item.bytes > 8 * 1024 * 1024 * 1024
                || !hash_valid(&item.sha256)
            {
                return Err(invalid());
            }
            sum.checked_add(item.bytes).ok_or_else(invalid)
        })
}
fn text(field: &[u8]) -> Result<String> {
    let end = field.iter().position(|v| *v == 0).unwrap_or(field.len());
    if field[end..].iter().any(|v| *v != 0) {
        return Err(invalid());
    }
    String::from_utf8(field[..end].to_vec()).map_err(|_| invalid())
}
fn number(field: &[u8]) -> Result<u64> {
    if field.first().is_some_and(|v| v & 0x80 != 0) {
        if field[0] & 0x40 != 0 {
            return Err(invalid());
        }
        return field.iter().enumerate().try_fold(0u64, |n, (i, value)| {
            n.checked_mul(256)
                .and_then(|n| n.checked_add(u64::from(if i == 0 { value & 0x7f } else { *value })))
                .ok_or_else(invalid)
        });
    }
    let value = std::str::from_utf8(field)
        .map_err(|_| invalid())?
        .trim_matches(['\0', ' ']);
    if value.is_empty() {
        return Ok(0);
    }
    if !value.bytes().all(|v| (b'0'..=b'7').contains(&v)) {
        return Err(invalid());
    }
    u64::from_str_radix(value, 8).map_err(|_| invalid())
}
fn pax(data: &[u8]) -> Result<Option<u64>> {
    let mut at = 0;
    let mut size = None;
    let mut names = BTreeSet::new();
    while at < data.len() {
        let offset = data[at..]
            .iter()
            .position(|v| *v == b' ')
            .ok_or_else(invalid)?;
        let length = std::str::from_utf8(&data[at..at + offset])
            .map_err(|_| invalid())?
            .parse::<usize>()
            .map_err(|_| invalid())?;
        if length <= offset + 2
            || at.checked_add(length).is_none_or(|end| end > data.len())
            || data[at + length - 1] != b'\n'
        {
            return Err(invalid());
        }
        let record =
            std::str::from_utf8(&data[at + offset + 1..at + length - 1]).map_err(|_| invalid())?;
        let (key, value) = record.split_once('=').ok_or_else(invalid)?;
        if !names.insert(key) {
            return Err(invalid());
        }
        match key {
            "size" => {
                size = Some(value.parse::<u64>().map_err(|_| invalid())?);
            }
            "mtime" | "atime" | "ctime" | "uid" | "gid" => {
                if value.is_empty()
                    || !value
                        .bytes()
                        .all(|v| v.is_ascii_digit() || matches!(v, b'.' | b'-'))
                {
                    return Err(invalid());
                }
            }
            "uname" | "gname" => {
                if value.chars().any(char::is_control) {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()), // No path/link override or sparse archive interpretation.
        }
        at += length;
    }
    Ok(size)
}
fn read_exact(input: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    input.read_exact(bytes).map_err(|_| invalid())
}
fn pad(input: &mut impl Read, length: u64) -> Result<()> {
    let remainder = ((512 - length % 512) % 512) as usize;
    let mut bytes = [0u8; 512];
    read_exact(input, &mut bytes[..remainder])?;
    if bytes[..remainder].iter().any(|v| *v != 0) {
        return Err(invalid());
    }
    Ok(())
}
/// Admit only the trusted helper's exact authenticated inventory into NEW native files.
/// No tar-specified ACL, path override, link, executable or special file is adopted.
pub(super) fn unpack(mut input: impl Read, workspace: &Path, result: &HelperResult) -> Result<()> {
    let needed = inventory(result)?;
    if fs2::available_space(workspace).map_err(|_| err("STORAGE_UNAVAILABLE"))?
        < needed
            .checked_add(2 * 1024 * 1024 * 1024)
            .ok_or_else(invalid)?
    {
        return Err(err("STORAGE_QUOTA"));
    }
    let parent = HostDirectory::inspect(workspace)?;
    parent.check_private()?;
    let root = parent.create_directory("archive")?;
    let files = root.create_directory("files")?;
    let mut seen = BTreeSet::new();
    let mut directories = BTreeSet::new();
    let mut next_size = None;
    let mut metadata_count = 0usize;
    loop {
        let mut header = [0u8; 512];
        read_exact(&mut input, &mut header)?;
        if header.iter().all(|v| *v == 0) {
            let mut second = [0u8; 512];
            read_exact(&mut input, &mut second)?;
            if second.iter().any(|v| *v != 0) || next_size.is_some() {
                return Err(invalid());
            }
            let mut tail = [0u8; 4096];
            let mut padding = 0;
            loop {
                let n = input.read(&mut tail).map_err(|_| invalid())?;
                if n == 0 {
                    break;
                }
                padding += n;
                if padding > 65536 || tail[..n].iter().any(|v| *v != 0) {
                    return Err(invalid());
                }
            }
            break;
        }
        let expected_checksum = number(&header[148..156])?;
        let checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*v)
                }
            })
            .sum();
        if checksum != expected_checksum || &header[257..262] != b"ustar" {
            return Err(invalid());
        }
        let name = text(&header[..100])?;
        let prefix = text(&header[345..500])?;
        let name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let header_size = number(&header[124..136])?;
        let kind = header[156];
        if kind == b'x' {
            metadata_count += 1;
            if next_size.is_some()
                || metadata_count > result.archive_inventory.len() + 2
                || header_size > 4096
            {
                return Err(invalid());
            }
            let mut data = vec![0u8; header_size as usize];
            read_exact(&mut input, &mut data)?;
            next_size = pax(&data)?;
            pad(&mut input, header_size)?;
            continue;
        }
        let bytes = next_size.take().unwrap_or(header_size);
        if kind == b'5' {
            let directory = name.strip_suffix('/').unwrap_or(&name);
            if bytes != 0
                || !matches!(directory, "archive" | "archive/files")
                || !directories.insert(directory.to_owned())
            {
                return Err(invalid());
            }
            continue;
        }
        if !matches!(kind, 0 | b'0') || header[157..257].iter().any(|v| *v != 0) {
            return Err(invalid());
        }
        let name = name.strip_prefix("archive/").ok_or_else(invalid)?;
        let item = result.archive_inventory.get(name).ok_or_else(invalid)?;
        if !name_valid(name)
            || bytes != item.bytes
            || !seen.insert(name.to_owned())
            || number(&header[100..108])? & 0o7777 != 0o600
        {
            return Err(invalid());
        }
        let (directory, filename) = if let Some(filename) = name.strip_prefix("files/") {
            (&files, filename)
        } else {
            (&root, name)
        };
        let mut output = directory.create_file(filename)?;
        let mut remaining = bytes;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut since_space = 0u64;
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            read_exact(&mut input, &mut buffer[..count])?;
            output
                .write_all(&buffer[..count])
                .map_err(|_| err("BACKUP_WRITE_UNCERTAIN"))?;
            hash.update(&buffer[..count]);
            remaining -= count as u64;
            since_space += count as u64;
            if since_space >= 4 * 1024 * 1024 {
                if fs2::available_space(workspace).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                    < 2 * 1024 * 1024 * 1024
                {
                    return Err(err("STORAGE_QUOTA"));
                }
                since_space = 0;
            }
        }
        output.sync_all()?;
        output.check_private()?;
        if format!("{:x}", hash.finalize()) != item.sha256 {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
        drop(output);
        pad(&mut input, bytes)?;
    }
    if seen != result.archive_inventory.keys().cloned().collect() {
        return Err(invalid());
    }
    root.check_private()?;
    files.check_private()?;
    validate_native(&root, &files, result)
}
fn validate_native(
    root: &HostDirectory,
    files: &HostDirectory,
    result: &HelperResult,
) -> Result<()> {
    let mut actual = Vec::new();
    for item in fs::read_dir(root.path()).map_err(|_| invalid())? {
        let item = item.map_err(|_| invalid())?;
        let name = item.file_name().into_string().map_err(|_| invalid())?;
        if name == "files" {
            files.check_private()?;
            for item in fs::read_dir(files.path()).map_err(|_| invalid())? {
                actual.push(format!(
                    "files/{}",
                    item.map_err(|_| invalid())?
                        .file_name()
                        .into_string()
                        .map_err(|_| invalid())?
                ));
            }
        } else {
            actual.push(name);
        }
    }
    actual.sort();
    if actual != result.archive_inventory.keys().cloned().collect::<Vec<_>>() {
        return Err(invalid());
    }
    for (name, expected) in &result.archive_inventory {
        let (parent, name) = name
            .strip_prefix("files/")
            .map(|name| (files, name))
            .unwrap_or((root, name));
        let mut source = parent.child_file(name)?;
        source.check_private()?;
        let before = source.metadata()?;
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = source.read(&mut buffer).map_err(|_| invalid())?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > expected.bytes {
                return Err(invalid());
            }
            hash.update(&buffer[..n]);
        }
        source.check_private()?;
        if count != expected.bytes
            || before.len() != count
            || source.metadata()?.len() != count
            || format!("{:x}", hash.finalize()) != expected.sha256
        {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
    }
    root.check_private()?;
    files.check_private()?;
    Ok(())
}

const CONFIGURATION: [&str; 6] = [
    "manager-bundle-manifest.json",
    "manager-compose.yaml",
    "manager-runtime.env",
    "manager-installed.json",
    "manager-engine.json",
    "manager-image-inventory.json",
];
struct Source {
    file: HostFile,
    expected: FileHash,
    before: std::fs::Metadata,
    permissions: crate::windows_private::HostPermissions,
}
fn unchanged(source: &Source) -> std::io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    source
        .file
        .check_private()
        .map_err(|_| std::io::Error::other("BACKUP_SOURCE_CHANGED"))?;
    let after = source
        .file
        .metadata()
        .map_err(|_| std::io::Error::other("BACKUP_SOURCE_CHANGED"))?;
    let same = |m: &std::fs::Metadata| {
        (
            m.len(),
            m.creation_time(),
            m.last_write_time(),
            m.file_attributes(),
        )
    };
    if source
        .file
        .permissions()
        .map_err(|_| std::io::Error::other("BACKUP_SOURCE_CHANGED"))?
        != source.permissions
        || same(&source.before) != same(&after)
    {
        return Err(std::io::Error::other("BACKUP_SOURCE_CHANGED"));
    }
    Ok(())
}
/// Finite, pinned native input; every EOF authenticates bytes and all retained sources.
struct Inputs {
    header: std::io::Cursor<Vec<u8>>,
    sources: Vec<Source>,
    index: usize,
    read: u64,
    hash: Sha256,
    archive: Option<ArchiveGuard>,
}
impl Read for Inputs {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let n = self.header.read(buffer)?;
        if n != 0 {
            return Ok(n);
        }
        loop {
            if self.index == self.sources.len() {
                for source in &self.sources {
                    unchanged(source)?;
                }
                if let Some(archive) = &self.archive {
                    archive
                        .check()
                        .map_err(|_| std::io::Error::other("BACKUP_SOURCE_CHANGED"))?;
                }
                return Ok(0);
            }
            let source = &mut self.sources[self.index];
            let n = source.file.read(buffer)?;
            if n != 0 {
                self.read = self
                    .read
                    .checked_add(n as u64)
                    .ok_or_else(|| std::io::Error::other("BACKUP_SOURCE_CHANGED"))?;
                if self.read > source.expected.bytes {
                    return Err(std::io::Error::other("BACKUP_SOURCE_CHANGED"));
                }
                self.hash.update(&buffer[..n]);
                return Ok(n);
            }
            unchanged(source)?;
            if self.read != source.expected.bytes
                || format!("{:x}", self.hash.clone().finalize()) != source.expected.sha256
            {
                return Err(std::io::Error::other("BACKUP_SOURCE_CHANGED"));
            }
            self.index += 1;
            self.read = 0;
            self.hash = Sha256::new();
        }
    }
}
pub(super) fn inputs(
    profile: &Path,
    key: &Path,
    preparation: &Path,
    deployment: &Path,
    images: &BTreeMap<String, FileHash>,
) -> Result<(Box<dyn Read + Send>, u64)> {
    let key_bytes = crate::windows_private::read_external_private_key(key, profile)?;
    let mut specifications = vec![(
        "key".to_owned(),
        key.to_owned(),
        FileHash {
            bytes: 32,
            sha256: format!("{:x}", Sha256::digest(key_bytes)),
        },
    )];
    for name in CONFIGURATION {
        let path = preparation.join(name);
        let expected = hash_file(&path)?;
        if expected.bytes > 1024 * 1024 {
            return Err(invalid());
        }
        specifications.push((format!("installation/{name}"), path, expected));
    }
    if images.len() != 2 {
        return Err(invalid());
    }
    for (name, expected) in images {
        if !matches!(name.as_str(), "image-0.tar" | "image-1.tar") {
            return Err(invalid());
        }
        specifications.push((
            format!("deployment/{name}"),
            deployment.join(name),
            expected.clone(),
        ));
    }
    guarded_inputs(specifications)
}
/// Shared only by the original create/verify/restore Linux helper consumers.
/// Caller validates external-key overlap and its operation's closed inventory.
pub(crate) fn guarded_inputs(
    specifications: Vec<(String, PathBuf, FileHash)>,
) -> Result<(Box<dyn Read + Send>, u64)> {
    make_inputs(specifications, None)
}
fn make_inputs(
    specifications: Vec<(String, PathBuf, FileHash)>,
    archive: Option<ArchiveGuard>,
) -> Result<(Box<dyn Read + Send>, u64)> {
    if specifications.is_empty() || specifications.len() > 10004 {
        return Err(invalid());
    }
    let mut descriptors = Vec::new();
    let mut sources = Vec::new();
    let mut total = 0u64;
    let mut names = BTreeSet::new();
    for (name, path, expected) in specifications {
        let permitted = name == "key"
            || name.strip_prefix("archive/").is_some_and(name_valid)
            || name
                .strip_prefix("installation/")
                .is_some_and(|n| CONFIGURATION.contains(&n))
            || matches!(
                name.as_str(),
                "deployment/image-0.tar" | "deployment/image-1.tar"
            );
        if !permitted
            || !names.insert(name.clone())
            || expected.bytes == 0
            || expected.bytes > 8 * 1024 * 1024 * 1024
            || !hash_valid(&expected.sha256)
            || (name == "key" && expected.bytes != 32)
        {
            return Err(invalid());
        }
        let file = HostFile::open(&path)?;
        file.check_private()?;
        let before = file.metadata()?;
        let permissions = file.permissions()?;
        if before.len() != expected.bytes {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
        total = total.checked_add(expected.bytes).ok_or_else(invalid)?;
        descriptors.push(
            serde_json::json!({"name": name, "bytes": expected.bytes, "sha256": expected.sha256}),
        );
        sources.push(Source {
            file,
            expected,
            before,
            permissions,
        });
    }
    if !names.contains("key") {
        return Err(invalid());
    }
    let json =
        serde_json::to_vec(&serde_json::json!({"files": descriptors})).map_err(|_| invalid())?;
    // Original manifest cap: do not narrow the existing 10,000-member archive.
    if json.len() > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    let mut header = (json.len() as u64).to_be_bytes().to_vec();
    header.extend(json);
    total = total.checked_add(header.len() as u64).ok_or_else(invalid)?;
    Ok((
        Box::new(Inputs {
            header: std::io::Cursor::new(header),
            sources,
            index: 0,
            read: 0,
            hash: Sha256::new(),
            archive,
        }),
        total,
    ))
}
struct ArchiveGuard {
    root: HostDirectory,
    files: HostDirectory,
    names: Vec<String>,
}
impl ArchiveGuard {
    fn census(&self) -> Result<Vec<String>> {
        self.root.check_private()?;
        self.files.check_private()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(self.root.path()).map_err(|_| invalid())? {
            let name = entry
                .map_err(|_| invalid())?
                .file_name()
                .into_string()
                .map_err(|_| invalid())?;
            if name == "files" {
                for entry in fs::read_dir(self.files.path()).map_err(|_| invalid())? {
                    let name = format!(
                        "files/{}",
                        entry
                            .map_err(|_| invalid())?
                            .file_name()
                            .into_string()
                            .map_err(|_| invalid())?
                    );
                    if !name_valid(&name) || names.len() >= 10003 {
                        return Err(invalid());
                    }
                    names.push(name);
                }
            } else {
                if !["complete.json", "writing.json", "manifest.gcm"].contains(&name.as_str()) {
                    return Err(invalid());
                }
                names.push(name);
            }
        }
        names.sort();
        if names.len() < 4
            || names.len() > 10003
            || !["complete.json", "writing.json", "manifest.gcm"]
                .iter()
                .all(|n| names.iter().any(|v| v.as_str() == *n))
        {
            return Err(invalid());
        }
        Ok(names)
    }
    fn check(&self) -> Result<()> {
        if self.census()? != self.names {
            return Err(err("BACKUP_SOURCE_CHANGED"));
        }
        Ok(())
    }
}
pub(crate) fn archive_inputs(
    profile: &Path,
    key: &Path,
    archive: &Path,
) -> Result<(Box<dyn Read + Send>, u64)> {
    let mut key_bytes = crate::windows_private::read_external_private_key(key, profile)?;
    let key_hash = format!("{:x}", Sha256::digest(key_bytes));
    key_bytes.fill(0);
    // Native ancestor identity also refuses a key hidden inside the archive.
    let mut checked = crate::windows_private::read_external_private_key(key, archive)?;
    checked.fill(0);
    let root = HostDirectory::inspect(archive)?;
    root.check_private()?;
    let files = root.child_directory("files")?;
    files.check_private()?;
    let mut guard = ArchiveGuard {
        root,
        files,
        names: Vec::new(),
    };
    guard.names = guard.census()?;
    let mut specifications = vec![(
        "key".to_owned(),
        key.to_owned(),
        FileHash {
            bytes: 32,
            sha256: key_hash,
        },
    )];
    for name in &guard.names {
        specifications.push((
            format!("archive/{name}"),
            archive.join(name),
            hash_file(&archive.join(name))?,
        ));
    }
    make_inputs(specifications, Some(guard))
}

const BOOTSTRAP: &str = r#"
import {mkdir as nativeMkdir,open as nativeOpen,statfs as nativeStatfs} from 'node:fs/promises';
import {createHash as nativeHash} from 'node:crypto';
{
 const input=process.stdin;
 function ready(){return new Promise((resolve,reject)=>{const done=()=>{clear();resolve();},failed=error=>{clear();reject(error);},clear=()=>{input.off('readable',done);input.off('end',done);input.off('error',failed);};input.once('readable',done);input.once('end',done);input.once('error',failed);if(input.readableEnded)done();else if(input.errored)failed(input.errored);});}
 async function take(n,eof=false){for(;;){const chunk=input.read(Math.min(n,65536));if(chunk!==null)return chunk;if(input.errored)throw input.errored;if(input.readableEnded){if(eof)return null;throw Error('BACKUP_INPUT_TRUNCATED');}await ready();}}
 async function exact(n){const out=Buffer.alloc(n);let at=0;while(at<n){const chunk=await take(n-at);chunk.copy(out,at);at+=chunk.length;}return out;}
 const length=(await exact(8)).readBigUInt64BE();if(length<1n||length>16384n)throw Error('BACKUP_INPUT_INVALID');
 const header=JSON.parse((await exact(Number(length))).toString('utf8'));
 const config=['manager-bundle-manifest.json','manager-compose.yaml','manager-runtime.env','manager-installed.json','manager-engine.json','manager-image-inventory.json'];
 const names=['key',...config.map(n=>'installation/'+n),'deployment/image-0.tar','deployment/image-1.tar'];
 if(Object.keys(header).join(',')!=='files'||!Array.isArray(header.files)||header.files.length!==names.length)throw Error('BACKUP_INPUT_INVALID');
 let total=0;for(let i=0;i<names.length;i++){const f=header.files[i];if(Object.keys(f).sort().join(',')!=='bytes,name,sha256'||f.name!==names[i]||!Number.isSafeInteger(f.bytes)||f.bytes<1||!(/^[0-9a-f]{64}$/).test(f.sha256)||(i===0?f.bytes!==32:i<=6?f.bytes>1048576:f.bytes>2147483648))throw Error('BACKUP_INPUT_INVALID');total+=f.bytes;}
 if(header.files.slice(7).reduce((n,f)=>n+f.bytes,0)>2147483648)throw Error('STORAGE_QUOTA');
 const disk=await nativeStatfs('/work',{bigint:true});if(disk.bavail*disk.bsize<BigInt(total)+2147483648n)throw Error('STORAGE_QUOTA');
 await nativeMkdir('/tmp/exhibitos-key',{mode:0o700});await nativeMkdir('/work/inputs',{mode:0o700});
 await nativeMkdir('/work/inputs/installation',{mode:0o700});await nativeMkdir('/work/inputs/deployment',{mode:0o700});
 for(const spec of header.files){const target=spec.name==='key'?'/tmp/exhibitos-key/key':'/work/inputs/'+spec.name;
  const out=await nativeOpen(target,'wx',0o600);try{const hash=nativeHash('sha256');let left=spec.bytes;while(left){const chunk=await take(Math.min(left,65536));let at=0;while(at<chunk.length){const written=await out.write(chunk,at,chunk.length-at,null);if(!written.bytesWritten)throw Error('BACKUP_INPUT_WRITE');at+=written.bytesWritten;}hash.update(chunk);left-=chunk.length;}
   await out.sync();const stat=await out.stat();if(stat.size!==spec.bytes||stat.nlink!==1||(stat.mode&0o7777)!==0o600||hash.digest('hex')!==spec.sha256)throw Error('BACKUP_INPUT_CHANGED');
  }finally{await out.close();}}
 if(await take(1,true)!==null)throw Error('BACKUP_INPUT_TRAILING');
}
"#;
pub(super) fn helper() -> String {
    format!(
        "{BOOTSTRAP}{}",
        HELPER.replace("'/backup-key'", "'/tmp/exhibitos-key/key'")
    )
}
/// Bounded output transport with the same operation cancellation fence and deadline.
pub(crate) fn stream<T: Send + 'static>(
    service: &LifecycleService,
    kind: &str,
    id: &str,
    args: &[String],
    consume: impl FnOnce(std::process::ChildStdout) -> Result<T> + Send + 'static,
) -> Result<T> {
    let executable = engine_executable("docker").ok_or_else(|| err("RUNTIME_MISSING"))?;
    let mut child = process_window::background_command(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| err("ENGINE_FAILED"))?;
    let stdout = child.stdout.take().ok_or_else(|| err("ENGINE_FAILED"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = sender.send(consume(stdout));
    });
    let started = std::time::Instant::now();
    let mut result = None;
    let outcome = loop {
        if let Err(error) = service.maintenance_checkpoint(kind, id) {
            break Err(error);
        }
        if started.elapsed().as_secs() >= 1800 {
            break Err(err("ENGINE_TIMEOUT"));
        }
        match receiver.try_recv() {
            Ok(Ok(value)) => result = Some(value),
            Ok(Err(error)) => break Err(error),
            Err(std::sync::mpsc::TryRecvError::Disconnected) if result.is_none() => {
                break Err(err("ENGINE_FAILED"));
            }
            _ => {}
        }
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => break Err(err("ENGINE_FAILED")),
            Ok(Some(_)) => {
                if let Some(value) = result.take() {
                    break Ok(value);
                }
            }
            Ok(None) => {}
            Err(_) => break Err(err("ENGINE_FAILED")),
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if outcome.is_err() {
        let _ = child.kill();
    }
    let waited = child.wait();
    let joined = worker.join();
    if waited.is_err() || joined.is_err() {
        return Err(err("ENGINE_FAILED"));
    }
    outcome
}
pub(super) fn save_image(
    service: &LifecycleService,
    id: &str,
    content: &str,
    path: &Path,
    remaining: u64,
) -> Result<FileHash> {
    let destination = path.to_owned();
    let hashed = stream(
        service,
        "backup",
        id,
        &["image".into(), "save".into(), content.into()],
        move |mut input| {
            let parent = HostDirectory::inspect(destination.parent().ok_or_else(invalid)?)?;
            parent.check_private()?;
            let mut out = parent.create_file(
                destination
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(invalid)?,
            )?;
            let mut count = 0u64;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let n = input.read(&mut buffer).map_err(|_| invalid())?;
                if n == 0 {
                    break;
                }
                count = count.checked_add(n as u64).ok_or_else(invalid)?;
                if count > remaining {
                    return Err(err("STORAGE_QUOTA"));
                }
                if fs2::available_space(parent.path()).map_err(|_| err("STORAGE_UNAVAILABLE"))?
                    < 2 * 1024 * 1024 * 1024
                {
                    return Err(err("STORAGE_QUOTA"));
                }
                out.write_all(&buffer[..n])
                    .map_err(|_| err("BACKUP_WRITE_UNCERTAIN"))?;
                hash.update(&buffer[..n]);
            }
            if count == 0 {
                return Err(invalid());
            }
            out.sync_all()?;
            out.check_private()?;
            parent.check_private()?;
            Ok(FileHash {
                bytes: count,
                sha256: format!("{:x}", hash.finalize()),
            })
        },
    )?;
    let checked = hash_file(path)?;
    if hashed.bytes != checked.bytes || hashed.sha256 != checked.sha256 {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    Ok(checked)
}
pub(super) fn copy_archive(
    service: &LifecycleService,
    id: &str,
    container: &str,
    volume: &str,
    image: &str,
    workspace: &Path,
    result: &HelperResult,
) -> Result<()> {
    let verify = || -> Result<()> {
        let value = inspected("docker", &["inspect".into(), container.into()])?;
        if value["Name"] != format!("/{container}")
            || value["Config"]["Labels"]["com.exhibitos.backup"] != id
            || value["Image"] != image
            || value["State"]["Running"] != false
            || value["State"]["ExitCode"] != 0
            || !value["Mounts"].as_array().is_some_and(|m| {
                m.iter().any(|m| {
                    m["Type"] == "volume" && m["Name"] == volume && m["Destination"] == "/work"
                })
            })
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        let work = inspected(
            "docker",
            &["volume".into(), "inspect".into(), volume.into()],
        )?;
        if work["Labels"]["com.exhibitos.backup"] != id
            || work["Driver"] != "local"
            || work["Options"].as_object().is_some_and(|o| !o.is_empty())
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        Ok(())
    };
    verify()?;
    let workspace = workspace.to_owned();
    let result = result.clone();
    stream(
        service,
        "backup",
        id,
        &[
            "cp".into(),
            format!("{container}:/work/archive"),
            "-".into(),
        ],
        move |input| unpack(input, &workspace, &result),
    )?;
    verify()
}

const ARCHIVE_BOOTSTRAP: &str = r#"
import {mkdir as archiveMkdir,open as archiveOpen,lstat as archiveLstat,readdir as archiveReaddir,statfs as archiveStatfs} from 'node:fs/promises';
import {constants as archiveFlags} from 'node:fs';
import {createHash as archiveHash} from 'node:crypto';
{
 const input=process.stdin;
 function ready(){return new Promise((resolve,reject)=>{const done=()=>{clear();resolve();},failed=error=>{clear();reject(error);},clear=()=>{input.off('readable',done);input.off('end',done);input.off('error',failed);};input.once('readable',done);input.once('end',done);input.once('error',failed);if(input.readableEnded)done();else if(input.errored)failed(input.errored);});}
 async function take(n,eof=false){for(;;){const chunk=input.read(Math.min(n,65536));if(chunk!==null)return chunk;if(input.errored)throw input.errored;if(input.readableEnded){if(eof)return null;throw Error('BACKUP_INPUT_TRUNCATED');}await ready();}}
 async function exact(n){const out=Buffer.alloc(n);let at=0;while(at<n){const chunk=await take(n-at);chunk.copy(out,at);at+=chunk.length;}return out;}
 const length=(await exact(8)).readBigUInt64BE();if(length<1n||length>16777216n)throw Error('BACKUP_INPUT_INVALID');
 const header=JSON.parse((await exact(Number(length))).toString('utf8'));
 if(Object.keys(header).join(',')!=='files'||!Array.isArray(header.files)||header.files.length<5||header.files.length>10004)throw Error('BACKUP_INPUT_INVALID');
 const names=new Set();let total=0;
 for(let i=0;i<header.files.length;i++){const f=header.files[i];
  if(Object.keys(f).sort().join(',')!=='bytes,name,sha256'||!Number.isSafeInteger(f.bytes)||f.bytes<1||f.bytes>8589934592||!(/^[0-9a-f]{64}$/).test(f.sha256)||names.has(f.name))throw Error('BACKUP_INPUT_INVALID');
  if(i===0?f.name!=='key'||f.bytes!==32:!(/^archive\/(?:complete\.json|writing\.json|manifest\.gcm|files\/[0-9]{8}\.gcm)$/).test(f.name))throw Error('BACKUP_INPUT_INVALID');
  names.add(f.name);total+=f.bytes;
 }
 for(const n of ['archive/complete.json','archive/writing.json','archive/manifest.gcm'])if(!names.has(n))throw Error('BACKUP_INPUT_INVALID');
 async function privateDirectory(path){try{await archiveMkdir(path,{mode:0o700});}catch(e){if(e.code!=='EEXIST')throw e;}const s=await archiveLstat(path);if(!s.isDirectory()||s.isSymbolicLink()||s.uid!==process.getuid()||(s.mode&0o7777)!==0o700)throw Error('BACKUP_INPUT_INVALID');}
 await privateDirectory('/tmp/exhibitos-key');
 let reuse=false;try{const s=await archiveLstat('/work/archive');reuse=true;}catch(e){if(e.code!=='ENOENT')throw e;}
 if(!reuse){const disk=await archiveStatfs('/work',{bigint:true});if(disk.bavail*disk.bsize<BigInt(total)+2147483648n)throw Error('STORAGE_QUOTA');}
 await privateDirectory('/work/archive');await privateDirectory('/work/archive/files');
 for(const spec of header.files){const path=spec.name==='key'?'/tmp/exhibitos-key/key':'/work/'+spec.name;
  const existing=reuse&&spec.name!=='key';const out=await archiveOpen(path,existing?archiveFlags.O_RDONLY|archiveFlags.O_NOFOLLOW|archiveFlags.O_NONBLOCK:archiveFlags.O_WRONLY|archiveFlags.O_CREAT|archiveFlags.O_EXCL|archiveFlags.O_NOFOLLOW,0o600);
  try{const before=await out.stat({bigint:true});if(!before.isFile()||before.nlink!==1n||before.uid!==BigInt(process.getuid())||(before.mode&4095n)!==384n||existing&&before.size!==BigInt(spec.bytes))throw Error('BACKUP_INPUT_INVALID');
   const incoming=archiveHash('sha256'),stored=archiveHash('sha256');let left=spec.bytes;
   while(left){const chunk=await take(Math.min(left,65536));incoming.update(chunk);
    if(existing){const bytes=Buffer.alloc(chunk.length);let at=0;while(at<bytes.length){const got=await out.read(bytes,at,bytes.length-at,null);if(!got.bytesRead)throw Error('BACKUP_INPUT_CHANGED');at+=got.bytesRead;}stored.update(bytes);}
    else{let at=0;while(at<chunk.length){const got=await out.write(chunk,at,chunk.length-at,null);if(!got.bytesWritten)throw Error('BACKUP_INPUT_WRITE');at+=got.bytesWritten;}}
    left-=chunk.length;
   }
   if(!existing)await out.sync();
   const after=await out.stat({bigint:true}),current=await archiveLstat(path,{bigint:true});
   if(!after.isFile()||after.nlink!==1n||after.uid!==BigInt(process.getuid())||(after.mode&4095n)!==384n||after.size!==BigInt(spec.bytes)||current.dev!==after.dev||current.ino!==after.ino||current.isSymbolicLink()||incoming.digest('hex')!==spec.sha256||existing&&(stored.digest('hex')!==spec.sha256||before.dev!==after.dev||before.ino!==after.ino||before.size!==after.size||before.mtimeNs!==after.mtimeNs||before.ctimeNs!==after.ctimeNs||before.mode!==after.mode||before.uid!==after.uid||before.gid!==after.gid))throw Error('BACKUP_INPUT_CHANGED');
  }finally{await out.close();}
 }
 if(await take(1,true)!==null)throw Error('BACKUP_INPUT_TRAILING');
 const actual=[...(await archiveReaddir('/work/archive')).filter(n=>n!=='files').map(n=>'archive/'+n),...(await archiveReaddir('/work/archive/files')).map(n=>'archive/files/'+n)].sort();
 if(JSON.stringify(actual)!==JSON.stringify([...names].filter(n=>n!=='key').sort()))throw Error('BACKUP_INPUT_CHANGED');
 await privateDirectory('/work/archive');await privateDirectory('/work/archive/files');
}
"#;
pub(crate) fn bootstrap_archive(script: &str) -> String {
    format!(
        "{ARCHIVE_BOOTSTRAP}{}",
        script
            .replace("'/key'", "'/tmp/exhibitos-key/key'")
            .replace("'/backup-key'", "'/tmp/exhibitos-key/key'")
            .replace("'/archive'", "'/work/archive'")
            .replace("'/backup-source'", "'/work/archive'")
    )
}
fn selected_name(name: &str) -> bool {
    name == "authenticated/manifest.json"
        || name
            .strip_prefix("configuration/")
            .is_some_and(|n| CONFIGURATION.contains(&n) || n == "freeze-signing-key.json")
        || matches!(
            name,
            "deployment/images/image-0.tar" | "deployment/images/image-1.tar"
        )
}
fn unpack_selected(mut input: impl Read, destination: &Path, expected: &FileHash) -> Result<()> {
    let parent = HostDirectory::inspect(destination.parent().ok_or_else(invalid)?)?;
    parent.check_private()?;
    let filename = destination
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(invalid)?;
    let mut size_override = None;
    let mut metadata_count = 0;
    let header = loop {
        let mut header = [0u8; 512];
        read_exact(&mut input, &mut header)?;
        let checksum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*v)
                }
            })
            .sum();
        if number(&header[148..156])? != checksum || &header[257..262] != b"ustar" {
            return Err(invalid());
        }
        if header[156] != b'x' {
            break header;
        }
        metadata_count += 1;
        let bytes = number(&header[124..136])?;
        if metadata_count > 1 || bytes > 4096 {
            return Err(invalid());
        }
        let mut data = vec![0u8; bytes as usize];
        read_exact(&mut input, &mut data)?;
        size_override = pax(&data)?;
        pad(&mut input, bytes)?;
    };
    if !matches!(header[156], 0 | b'0')
        || text(&header[..100])? != filename
        || !text(&header[345..500])?.is_empty()
        || header[157..257].iter().any(|v| *v != 0)
        || number(&header[100..108])? & 0o7777 != 0o600
        || size_override.unwrap_or(number(&header[124..136])?) != expected.bytes
    {
        return Err(invalid());
    }
    let mut output = parent.create_file(filename)?;
    let mut hash = Sha256::new();
    let mut left = expected.bytes;
    let mut buffer = [0u8; 65536];
    while left != 0 {
        let n = left.min(buffer.len() as u64) as usize;
        read_exact(&mut input, &mut buffer[..n])?;
        if fs2::available_space(parent.path()).map_err(|_| err("STORAGE_UNAVAILABLE"))?
            < 2 * 1024 * 1024 * 1024
        {
            return Err(err("STORAGE_QUOTA"));
        }
        output
            .write_all(&buffer[..n])
            .map_err(|_| err("BACKUP_WRITE_UNCERTAIN"))?;
        hash.update(&buffer[..n]);
        left -= n as u64;
    }
    pad(&mut input, expected.bytes)?;
    let mut end = [0u8; 1024];
    read_exact(&mut input, &mut end)?;
    if end.iter().any(|v| *v != 0) {
        return Err(invalid());
    }
    let mut padding = 0;
    loop {
        let n = input.read(&mut buffer).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        padding += n;
        if padding > 65536 || buffer[..n].iter().any(|v| *v != 0) {
            return Err(invalid());
        }
    }
    output.sync_all()?;
    output.check_private()?;
    parent.check_private()?;
    if format!("{:x}", hash.finalize()) != expected.sha256 {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    drop(output);
    let checked = hash_file(destination)?;
    if checked.bytes != expected.bytes || checked.sha256 != expected.sha256 {
        return Err(err("BACKUP_SOURCE_CHANGED"));
    }
    Ok(())
}
/// Inventory is supplied only after original service-backup authentication.
/// Selected existing recovery paths are copied without admitting unrelated /work data.
pub(crate) fn copy_selected(
    service: &LifecycleService,
    kind: &str,
    id: &str,
    container: &str,
    label_key: &str,
    volume: &str,
    image: &str,
    workspace: &Path,
    inventory: &BTreeMap<String, FileHash>,
) -> Result<()> {
    if inventory.is_empty()
        || inventory.len() > 10
        || !matches!(kind, "backup" | "restoration")
        || label_key != format!("com.exhibitos.{kind}")
    {
        return Err(invalid());
    }
    let mut needed = 0u64;
    for (name, item) in inventory {
        if !selected_name(name)
            || item.bytes == 0
            || item.bytes > 8 * 1024 * 1024 * 1024
            || !hash_valid(&item.sha256)
        {
            return Err(invalid());
        }
        needed = needed.checked_add(item.bytes).ok_or_else(invalid)?;
    }
    if fs2::available_space(workspace).map_err(|_| err("STORAGE_UNAVAILABLE"))?
        < needed
            .checked_add(2 * 1024 * 1024 * 1024)
            .ok_or_else(invalid)?
    {
        return Err(err("STORAGE_QUOTA"));
    }
    let verify = || -> Result<()> {
        let helper = inspected("docker", &["inspect".into(), container.into()])?;
        if helper["Name"]
            != format!(
                "/exhibitos-{}-{id}",
                if kind == "restoration" {
                    "restore"
                } else {
                    "backup"
                }
            )
            || helper["Config"]["Labels"][label_key] != id
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
        let work = inspected(
            "docker",
            &["volume".into(), "inspect".into(), volume.into()],
        )?;
        if work["Labels"][label_key] != id
            || work["Driver"] != "local"
            || work["Options"].as_object().is_some_and(|v| !v.is_empty())
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        Ok(())
    };
    verify()?;
    for (name, item) in inventory {
        service.maintenance_checkpoint(kind, id)?;
        let destination = workspace.join(name);
        let expected = item.clone();
        stream(
            service,
            kind,
            id,
            &["cp".into(), format!("{container}:/work/{name}"), "-".into()],
            move |input| unpack_selected(input, &destination, &expected),
        )?;
        verify()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        crate::windows_private::PrivateDirectory,
        HelperResult,
        Vec<u8>,
    ) {
        let directory = crate::windows_private::PrivateDirectory::create(
            &std::env::temp_dir().join(format!("native-cipher-copy-{}", Uuid::new_v4())),
        )
        .unwrap();
        let mut inventory = BTreeMap::new();
        let mut tar = Vec::new();
        for (name, bytes) in [
            ("complete.json", b"synthetic complete".as_slice()),
            ("writing.json", b"synthetic journal".as_slice()),
            ("manifest.gcm", b"synthetic encrypted manifest".as_slice()),
            (
                "files/00000000.gcm",
                b"synthetic encrypted payload".as_slice(),
            ),
        ] {
            inventory.insert(
                name.to_owned(),
                FileHash {
                    bytes: bytes.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                },
            );
            entry(&mut tar, &format!("archive/{name}"), bytes);
        }
        tar.extend([0u8; 1024]);
        (
            directory,
            HelperResult {
                operation: "created-and-authenticated".into(),
                backup_id: Uuid::new_v4().to_string(),
                files: 1,
                authenticated_manifest_sha256: "a".repeat(64),
                archive_inventory: inventory,
            },
            tar,
        )
    }
    fn entry(tar: &mut Vec<u8>, name: &str, bytes: &[u8]) {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000600\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", bytes.len()).as_bytes());
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        let checksum: u64 = header.iter().map(|b| u64::from(*b)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        tar.extend(header);
        tar.extend(bytes);
        tar.resize(tar.len() + (512 - bytes.len() % 512) % 512, 0);
    }
    #[test]
    fn native_cipher_copy_preserves_exact_inventory_private_acl_and_refuses_overwrite() {
        let (directory, result, tar) = fixture();
        unpack(std::io::Cursor::new(&tar), directory.path(), &result).unwrap();
        validate_copy(&directory.path().join("archive"), &result).unwrap();
        let before = fs::read(directory.path().join("archive/manifest.gcm")).unwrap();
        assert!(unpack(std::io::Cursor::new(&tar), directory.path(), &result).is_err());
        assert_eq!(
            fs::read(directory.path().join("archive/manifest.gcm")).unwrap(),
            before
        );
        assert_eq!(
            hash_file(&directory.path().join("archive/files/00000000.gcm"))
                .unwrap()
                .sha256,
            result.archive_inventory["files/00000000.gcm"].sha256
        );
    }
    #[test]
    fn native_cipher_copy_rejects_unauthenticated_bytes_missing_inventory_and_path_escape() {
        let (directory, result, mut tar) = fixture();
        tar[512] ^= 1;
        assert!(unpack(std::io::Cursor::new(tar), directory.path(), &result).is_err());
        let (directory, mut result, tar) = fixture();
        result.archive_inventory.remove("manifest.gcm");
        assert!(unpack(std::io::Cursor::new(tar), directory.path(), &result).is_err());
        let (directory, result, _) = fixture();
        let mut escaped = Vec::new();
        entry(&mut escaped, "archive/../outside.gcm", b"synthetic");
        escaped.extend([0u8; 1024]);
        assert!(unpack(std::io::Cursor::new(escaped), directory.path(), &result).is_err());
        assert!(!directory.path().join("outside.gcm").exists());
    }
    #[test]
    fn native_input_eof_rechecks_full_source_hash_and_retains_writer_fence() {
        let directory = crate::windows_private::PrivateDirectory::create(
            &std::env::temp_dir().join(format!("native-input-{}", Uuid::new_v4())),
        )
        .unwrap();
        let mut writer = directory.create_record("source.bin").unwrap();
        writer.write_all(b"synthetic private bytes").unwrap();
        writer.sync_all().unwrap();
        drop(writer);
        let path = directory.path().join("source.bin");
        let expected = hash_file(&path).unwrap();
        let file = HostFile::open(&path).unwrap();
        let before = file.metadata().unwrap();
        let mut input = Inputs {
            header: std::io::Cursor::new(Vec::new()),
            sources: vec![Source {
                permissions: file.permissions().unwrap(),
                file,
                expected: expected.clone(),
                before,
            }],
            index: 0,
            read: 0,
            hash: Sha256::new(),
            archive: None,
        };
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"synthetic private bytes");
        drop(input);
        let file = HostFile::open(&path).unwrap();
        let before = file.metadata().unwrap();
        let mut wrong = expected;
        wrong.sha256 = "a".repeat(64);
        let mut input = Inputs {
            header: std::io::Cursor::new(Vec::new()),
            sources: vec![Source {
                permissions: file.permissions().unwrap(),
                file,
                expected: wrong,
                before,
            }],
            index: 0,
            read: 0,
            hash: Sha256::new(),
            archive: None,
        };
        assert!(input.read_to_end(&mut Vec::new()).is_err());
    }
    #[test]
    fn native_archive_inputs_retain_exact_private_namespace_and_refuse_added_member() {
        let (directory, result, tar) = fixture();
        unpack(std::io::Cursor::new(tar), directory.path(), &result).unwrap();
        let profile = crate::windows_private::PrivateDirectory::create(
            &std::env::temp_dir().join(format!("native-input-profile-{}", Uuid::new_v4())),
        )
        .unwrap();
        let external = crate::windows_private::PrivateDirectory::create(
            &std::env::temp_dir().join(format!("native-input-key-{}", Uuid::new_v4())),
        )
        .unwrap();
        let mut key = external.create_record("external.key").unwrap();
        key.write_all(&[19; 32]).unwrap();
        key.sync_all().unwrap();
        drop(key);
        let archive = directory.path().join("archive");
        let (mut reader, limit) = archive_inputs(
            profile.path(),
            &external.path().join("external.key"),
            &archive,
        )
        .unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, limit);
        drop(reader);
        let (mut reader, _) = archive_inputs(
            profile.path(),
            &external.path().join("external.key"),
            &archive,
        )
        .unwrap();
        let files =
            crate::windows_private::PrivateDirectory::inspect(&archive.join("files")).unwrap();
        let mut added = files.create_record("00000001.gcm").unwrap();
        added.write_all(b"unexpected synthetic member").unwrap();
        added.sync_all().unwrap();
        drop(added);
        assert!(reader.read_to_end(&mut Vec::new()).is_err());
    }
    #[test]
    fn native_selected_plaintext_copy_requires_authenticated_bytes_and_no_overwrite() {
        let directory = crate::windows_private::PrivateDirectory::create(
            &std::env::temp_dir().join(format!("native-selected-{}", Uuid::new_v4())),
        )
        .unwrap();
        let bytes = b"synthetic authenticated manifest";
        let expected = FileHash {
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        let mut tar = Vec::new();
        entry(&mut tar, "manifest.json", bytes);
        tar.extend([0u8; 1024]);
        let target = directory.path().join("manifest.json");
        unpack_selected(std::io::Cursor::new(&tar), &target, &expected).unwrap();
        assert_eq!(hash_file(&target).unwrap().sha256, expected.sha256);
        assert!(unpack_selected(std::io::Cursor::new(&tar), &target, &expected).is_err());
        assert_eq!(fs::read(&target).unwrap(), bytes);
        let bad_directory = crate::windows_private::PrivateDirectory::create(
            &std::env::temp_dir().join(format!("native-selected-corrupt-{}", Uuid::new_v4())),
        )
        .unwrap();
        let mut bad = tar;
        bad[512] ^= 1;
        assert!(
            unpack_selected(
                std::io::Cursor::new(bad),
                &bad_directory.path().join("manifest.json"),
                &expected
            )
            .is_err()
        );
        assert!(!selected_name("configuration/../../outside"));
        assert!(!selected_name("authenticated/00000000.plain"));
    }
}
