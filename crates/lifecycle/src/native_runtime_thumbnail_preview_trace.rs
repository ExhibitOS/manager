// SPDX-License-Identifier: Apache-2.0
//! One bounded observation from the inherited thumbnail preview collector.
use super::{Result, err};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{PipeReader, Read},
    os::fd::{AsRawFd, FromRawFd},
};
use uuid::{Uuid, Variant};

pub(super) const ERROR: &str = "NATIVE_THUMBNAIL_TRACE_UNAVAILABLE";
const LIMIT: usize = 2048;
const MAX_ELAPSED_MS: u32 = 60_000;

pub(super) fn request_id(value: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(value).ok()?;
    (id.get_variant() == Variant::RFC4122 && id.get_version_num() == 4 && id.to_string() == value)
        .then_some(id)
}

pub(super) struct Reader {
    pipe: PipeReader,
    request_id: Uuid,
}

pub(super) struct Writer {
    pipe: File,
    request_id: Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Bootstrap {
    version: u8,
    request_id: Uuid,
    output_fd: i32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Report {
    version: u8,
    request_id: String,
    anchor_unix_ms: u64,
    events: Vec<Event>,
}

#[derive(Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    ProxyIngress,
    ApiIngress,
    ApiAuthorized,
    ApiHeadersReady,
    ProxyUpstreamHeaders,
    ProxyHeadersForwarded,
    ProxyClientClosed,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Event {
    kind: Kind,
    elapsed_ms: u32,
    #[serde(
        default,
        deserialize_with = "present_status",
        skip_serializing_if = "Option::is_none"
    )]
    status: Option<u16>,
}

fn present_status<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<u16>, D::Error> {
    u16::deserialize(decoder).map(Some)
}

pub(super) fn pipe(id: Uuid) -> Result<(Reader, Writer)> {
    let (reader, writer) = std::io::pipe().map_err(|_| err(ERROR))?;
    for descriptor in [reader.as_raw_fd(), writer.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(err(ERROR));
        }
    }
    let descriptor = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if descriptor < 0 {
        return Err(err(ERROR));
    }
    let writer = unsafe { File::from_raw_fd(descriptor) };
    if descriptor > 1023 {
        return Err(err(ERROR));
    }
    Ok((
        Reader {
            pipe: reader,
            request_id: id,
        },
        Writer {
            pipe: writer,
            request_id: id,
        },
    ))
}

impl Reader {
    // Called only after owned shutdown. EOF is required; open, overlong,
    // truncated or nonconforming channels yield no report and never delay stop.
    pub(super) fn collect(self) -> Option<Report> {
        let mut bytes = Vec::with_capacity(LIMIT + 1);
        self.pipe
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        validate(&bytes, self.request_id)
    }
}

impl Writer {
    pub(super) fn fd(&self) -> i32 {
        self.pipe.as_raw_fd()
    }

    pub(super) fn bootstrap(&self) -> Bootstrap {
        Bootstrap {
            version: 1,
            request_id: self.request_id,
            output_fd: self.fd(),
        }
    }
}

fn validate(bytes: &[u8], id: Uuid) -> Option<Report> {
    if bytes.len() > LIMIT {
        return None;
    }
    let line = bytes.strip_suffix(b"\n")?;
    if line.contains(&b'\n') {
        return None;
    }
    let report: Report = serde_json::from_slice(line).ok()?;
    if report.version != 1
        || request_id(&report.request_id) != Some(id)
        || !(1..=9_007_199_254_740_991).contains(&report.anchor_unix_ms)
        || report.events.len() > 7
        || report
            .events
            .first()
            .is_some_and(|event| event.kind != Kind::ProxyIngress)
    {
        return None;
    }
    for (index, event) in report.events.iter().enumerate() {
        let earlier = &report.events[..index];
        let reports_status = matches!(
            event.kind,
            Kind::ApiHeadersReady | Kind::ProxyUpstreamHeaders | Kind::ProxyHeadersForwarded
        );
        if event.elapsed_ms > MAX_ELAPSED_MS
            || earlier.iter().any(|other| other.kind == event.kind)
            || earlier
                .last()
                .is_some_and(|other| other.elapsed_ms > event.elapsed_ms)
            || event.status.is_some() != reports_status
            || event
                .status
                .is_some_and(|status| !(100..=599).contains(&status))
        {
            return None;
        }
    }
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn id() -> Uuid {
        request_id("09a8fdc8-0b72-4db7-85ea-ec9098a01ea3").unwrap()
    }

    #[test]
    fn pipe_accepts_only_one_closed_matching_bounded_report() {
        let id = id();
        let data = format!(
            r#"{{"version":1,"requestId":"{id}","anchorUnixMs":123,"events":[{{"kind":"proxy-ingress","elapsedMs":3}},{{"kind":"proxy-headers-forwarded","elapsedMs":8,"status":200}}]}}"#
        ) + "\n";
        let (reader, mut writer) = pipe(id).unwrap();
        let flags = unsafe { libc::fcntl(writer.fd(), libc::F_GETFL) };
        assert!(flags >= 0 && (flags & libc::O_NONBLOCK) != 0);
        assert_eq!(flags & libc::O_ACCMODE, libc::O_WRONLY);
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        assert_eq!(unsafe { libc::fstat(writer.fd(), stat.as_mut_ptr()) }, 0);
        assert_eq!(
            unsafe { stat.assume_init() }.st_mode & libc::S_IFMT,
            libc::S_IFIFO
        );
        writer.pipe.write_all(data.as_bytes()).unwrap();
        drop(writer);
        assert!(reader.collect().is_some());
        assert!(validate(data.as_bytes(), Uuid::new_v4()).is_none());
        assert!(validate(data.repeat(2).as_bytes(), id).is_none());
        assert!(validate(&[b' '; LIMIT + 1], id).is_none());
        assert!(validate(data.trim_end().as_bytes(), id).is_none());
        assert!(validate(data.replace("status", "url").as_bytes(), id).is_none());
        assert!(validate(data.replace("200", "600").as_bytes(), id).is_none());
        assert!(
            validate(
                data.replace("elapsedMs\":8", "elapsedMs\":2").as_bytes(),
                id
            )
            .is_none()
        );
        let (reader, _writer) = pipe(id).unwrap();
        assert!(reader.collect().is_none());
    }
}
