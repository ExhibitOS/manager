// SPDX-License-Identifier: Apache-2.0
//! Bounded, transient stderr parsing. Only fixed classes leave this module.
use serde::Serialize;
use std::io::Read;

const LINE_LIMIT: usize = 256;
const DRAIN_LIMIT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum ChildError {
    Unknown,
    StaticFetch,
    StaticFileTooLarge,
    StaticJsonSize,
    StaticJsonInvalid,
    NativeDetailPlan,
    StaticPublicationChanged,
    StaticPathCollision,
    StaticFileCount,
    NativeDetailStage,
    NativeDetailFrame,
    NativeDetailUnavailable,
    NativeDetailPipe,
    NativeDetailControl,
    NativeDetailReceipt,
    NativeDetailCancelled,
}

fn classify(message: &[u8], publication: &str) -> ChildError {
    use ChildError::*;
    match message {
        b"STATIC_FETCH: publication.json" => StaticFetch,
        b"STATIC_FILE_TOO_LARGE: publication.json" |
        b"STATIC_FILE_TOO_LARGE: static-export-manifest.json" => StaticFileTooLarge,
        b"STATIC_JSON_SIZE" => StaticJsonSize,
        b"STATIC_JSON_INVALID" => StaticJsonInvalid,
        b"NATIVE_DETAIL_PLAN" => NativeDetailPlan,
        b"STATIC_PATH_COLLISION: static-export-manifest.json" => StaticPathCollision,
        b"STATIC_FILE_COUNT: static-export-manifest.json" => StaticFileCount,
        b"NATIVE_DETAIL_STAGE" => NativeDetailStage,
        b"NATIVE_DETAIL_FRAME" => NativeDetailFrame,
        b"NATIVE_DETAIL_UNAVAILABLE" => NativeDetailUnavailable,
        b"NATIVE_DETAIL_PIPE" => NativeDetailPipe,
        b"NATIVE_DETAIL_CONTROL" => NativeDetailControl,
        b"NATIVE_DETAIL_RECEIPT" => NativeDetailReceipt,
        b"NATIVE_DETAIL_CANCELLED" => NativeDetailCancelled,
        _ if crate::installations::uuid(publication)
            && message == format!("STATIC_PUBLICATION_CHANGED: {publication}").as_bytes()
            => StaticPublicationChanged,
        _ => Unknown,
    }
}

pub(super) struct Collector<R> {
    input: Option<R>,
    line: Vec<u8>,
    overflow: bool,
    pub(super) error: ChildError,
    primary_seen: bool,
    pub(super) truncated: bool,
}
impl<R: Read> Collector<R> {
    pub(super) fn new(input: Option<R>) -> Self {
        Self { input, line: Vec::with_capacity(LINE_LIMIT), overflow: false,
            error: ChildError::Unknown, primary_seen: false, truncated: false }
    }
    fn end_line(&mut self, publication: &str) {
        // The first primary Error line wins, including an unknown/overlong one.
        // Never search arbitrary stacks, suffixes, source lines or URL text.
        let primary = [b"Error".as_slice(), b"TypeError", b"RangeError", b"SyntaxError",
            b"ReferenceError", b"AggregateError", b"AbortError", b"URIError", b"EvalError"]
            .iter().any(|name| self.line.strip_prefix(*name).is_some_and(|suffix|
                suffix.starts_with(b": ") || suffix.starts_with(b" [")));
        if !self.primary_seen && primary {
            self.primary_seen = true;
            if !self.overflow && self.line.starts_with(b"Error: ") {
                let message = self.line.strip_suffix(b"\r").unwrap_or(&self.line);
                self.error = classify(&message[7..], publication);
            }
        }
        self.line.clear(); self.overflow = false;
    }
    pub(super) fn drain(&mut self, publication: &str) {
        let mut remaining = DRAIN_LIMIT;
        let mut bytes = [0; 1024];
        while remaining > 0 {
            let Some(input) = self.input.as_mut() else { return; };
            match input.read(&mut bytes[..remaining.min(1024)]) {
                Ok(0) => { self.end_line(publication); self.input = None; return; }
                Ok(n) => {
                    remaining -= n;
                    for byte in &bytes[..n] {
                        if *byte == b'\n' { self.end_line(publication); }
                        else if self.line.len() < LINE_LIMIT { self.line.push(*byte); }
                        else { self.overflow = true; self.truncated = true; }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => return,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return,
                Err(_) => { self.input = None; return; }
            }
        }
        self.truncated = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Phase { Startup, AwaitStaged, VerifyStaging, WriteAcknowledgement, AwaitReady, StdoutEnd, ChildExit }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Failure<'a> {
    pub(super) version: u8,
    pub(super) job_id: &'a str,
    pub(super) publication_id: &'a str,
    pub(super) runtime_generation: &'a str,
    pub(super) phase: Phase,
    pub(super) rust_error: RustError,
    pub(super) child_error: ChildError,
    pub(super) stderr_truncated: bool,
    // Observed before the existing cleanup; a later signal is not the cause.
    pub(super) exit_code: Option<i32>,
    pub(super) exit_signal: Option<i32>,
    pub(super) running: Option<bool>,
    pub(super) elapsed_ms: u64,
    pub(super) deadline_reached: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RustError { Timeout, Cancelled, Other }
impl RustError {
    pub(super) fn from_code(code: &str) -> Self {
        match code {
            "NATIVE_STATIC_DETAIL_TIMEOUT" => Self::Timeout,
            "NATIVE_STATIC_CANCELLED" => Self::Cancelled,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    const PUBLICATION: &str = "10000000-0000-4000-8000-000000000001";
    fn parsed(bytes: &[u8]) -> Collector<Cursor<&[u8]>> {
        let mut c = Collector::new(Some(Cursor::new(bytes))); c.drain(PUBLICATION); c
    }
    #[test]
    fn detail_diagnostic_exact_literals_and_current_publication_only() {
        assert_eq!(parsed(b"source\nError: STATIC_FETCH: publication.json\n stack").error, ChildError::StaticFetch);
        assert_eq!(parsed(format!("Error: STATIC_PUBLICATION_CHANGED: {PUBLICATION}").as_bytes()).error, ChildError::StaticPublicationChanged);
        for message in ["STATIC_FETCH: https://secret.invalid/token", "STATIC_JSON_INVALID suffix", "STATIC_PUBLICATION_CHANGED: 20000000-0000-4000-8000-000000000001", "STATIC_FUTURE"] {
            assert_eq!(parsed(format!("Error: {message}\n").as_bytes()).error, ChildError::Unknown);
        }
    }
    #[test]
    fn detail_diagnostic_does_not_promote_stack_or_second_error() {
        for bytes in [b"Error: fetch failed\nError: STATIC_JSON_INVALID\n".as_slice(), b"TypeError: fetch failed\nError: STATIC_JSON_INVALID\n", b"Error [ERR_X]: unknown\nError: STATIC_JSON_INVALID\n", b"TypeError [ERR_INVALID_URL]: private URL\nError: STATIC_JSON_INVALID\n", b"  Error: STATIC_JSON_INVALID\n", b"STATIC_JSON_INVALID\n", b"Error: \xff\n"] {
            assert_eq!(parsed(bytes).error, ChildError::Unknown);
        }
    }
    #[test]
    fn detail_diagnostic_bounds_overflow_and_rejects_truncated_primary() {
        let mut bytes = b"Error: STATIC_JSON_INVALID".to_vec(); bytes.extend(vec![b'x'; 100_000]);
        let mut c = parsed(&bytes); assert!(c.truncated); assert!(c.line.len() <= LINE_LIMIT);
        for _ in 0..8 { c.drain(PUBLICATION); }
        assert_eq!(c.error, ChildError::Unknown); assert!(c.line.is_empty());
    }
    #[test]
    fn detail_diagnostic_serialization_contains_only_fixed_classes() {
        let c = parsed(b"Error: fetch failed https://secret.invalid/token\n");
        let f = Failure { version: 1, job_id: PUBLICATION, publication_id: PUBLICATION,
            runtime_generation: PUBLICATION, phase: Phase::AwaitStaged,
            rust_error: RustError::from_code("arbitrary secret"), child_error: c.error,
            stderr_truncated: c.truncated, exit_code: Some(1), exit_signal: None,
            running: Some(false), elapsed_ms: 21, deadline_reached: false };
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("secret")); assert!(!json.contains("fetch"));
        assert!(json.contains("UNKNOWN")); assert!(json.contains("await-staged"));
    }
    #[test]
    fn detail_diagnostic_nonblocking_pipe_flood_keeps_stdout_separate() {
        use std::{os::fd::AsRawFd, process::{Command, Stdio}};
        let mut child = Command::new("/bin/sh").args(["-c", "printf 'protocol-only'; i=0; while [ $i -lt 4000 ]; do printf 'bounded synthetic diagnostic noise\\n' >&2; i=$((i+1)); done; printf 'Error: STATIC_JSON_INVALID\\n' >&2"])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let stderr = child.stderr.take().unwrap();
        super::super::detail_nonblocking(stderr.as_raw_fd()).unwrap();
        let mut c = Collector::new(Some(stderr));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while c.input.is_some() && std::time::Instant::now() < until {
            c.drain(PUBLICATION); std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let drained = c.input.is_none(); let error = c.error;
        assert!(c.line.len() <= LINE_LIMIT);
        // Closing stderr also lets a failed flood test reap a blocked writer.
        drop(c);
        let exit = child.wait().unwrap();
        assert!(drained); assert!(exit.success());
        let mut stdout = String::new(); child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
        assert_eq!(stdout, "protocol-only"); assert_eq!(error, ChildError::StaticJsonInvalid);
    }
}
