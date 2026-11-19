// SPDX-License-Identifier: Apache-2.0
//! One private, read-only loopback reader owned by the native host.
use super::*;
use crate::{
    installations::native_static_export::{Receipt, Status, verify_output},
    native_payload::Payload,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    viewer_id: String,
    url: String,
    authorization: String,
    revision_sha256: String,
}

pub(super) struct Viewer {
    id: String,
    child: Child,
}

fn unavailable() -> LifecycleError {
    err("NATIVE_STATIC_VIEWER_UNAVAILABLE")
}

fn completed_receipt<'a>(status: &'a Status, job_id: &str) -> Result<&'a Receipt> {
    status
        .receipt
        .as_ref()
        .filter(|receipt| {
            status.state == "completed"
                && status.job_id == job_id
                && status.error_code.is_none()
                && receipt.output == status.output
                && receipt.publication_id == status.publication_id
                && receipt.revision_sha256 == status.revision_sha256
        })
        .ok_or_else(|| err("NATIVE_STATIC_VIEWER_RECEIPT_INVALID"))
}

impl Viewer {
    pub fn open(payload: &Payload, status: &Status, job_id: &str) -> Result<(Self, Entry)> {
        let invalid = || err("NATIVE_STATIC_VIEWER_RECEIPT_INVALID");
        let receipt = completed_receipt(status, job_id)?;
        if &verify_output(status).map_err(|_| invalid())? != receipt {
            return Err(invalid());
        }

        let reader = payload.static_publication_reader()?;
        let id = Uuid::new_v4().to_string();
        let secret = Uuid::new_v4().simple().to_string();
        let child = Command::new(payload.node())
            .env_clear()
            .args([
                "--input-type=module",
                "-e",
                include_str!("native_static_viewer.mjs"),
            ])
            .arg(reader)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| unavailable())?;
        let mut viewer = Self {
            id: id.clone(),
            child,
        };
        let input = viewer.child.stdin.as_mut().ok_or_else(unavailable)?;
        serde_json::to_writer(
            &mut *input,
            &serde_json::json!({"receipt":receipt, "secret":secret}),
        )
        .map_err(|_| unavailable())?;
        input.write_all(b"\n").map_err(|_| unavailable())?;
        let output = viewer.child.stdout.take().ok_or_else(unavailable)?;
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let port = BufReader::new(output)
                .take(8)
                .read_line(&mut line)
                .ok()
                .and_then(|_| line.strip_suffix('\n'))
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|port| *port >= 1024);
            let _ = send.send(port);
        });
        let port = receive
            .recv_timeout(Duration::from_secs(30))
            .ok()
            .flatten()
            .ok_or_else(unavailable)?;
        let entry = Entry {
            viewer_id: id,
            url: format!("http://127.0.0.1:{port}/index.html"),
            authorization: format!("Bearer {secret}"),
            revision_sha256: receipt.revision_sha256.clone(),
        };
        Ok((viewer, entry))
    }

    pub fn matches(&self, id: &str) -> bool {
        self.id == id
    }

    pub fn close(&mut self) -> Result<()> {
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self.child.try_wait().map_err(|_| unavailable())?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                self.child.wait().map_err(|_| unavailable())?;
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_requires_the_same_completed_job_and_receipted_revision() {
        let receipt = Receipt {
            publication_id: Uuid::new_v4().to_string(),
            revision_sha256: "a".repeat(64),
            index_sha256: "b".repeat(64),
            manifest_sha256: "c".repeat(64),
            files: 2,
            bytes: 42,
            output: PathBuf::from("/exports/contained"),
        };
        let mut status = Status {
            format: 1,
            job_id: Uuid::new_v4().to_string(),
            workspace_id: Uuid::new_v4().to_string(),
            tenant_id: Uuid::new_v4().to_string(),
            principal_id: Uuid::new_v4().to_string(),
            publication_id: receipt.publication_id.clone(),
            revision_sha256: receipt.revision_sha256.clone(),
            output: receipt.output.clone(),
            output_parent_identity: None,
            state: "completed".into(),
            private_editor: false,
            receipt: Some(receipt),
            error_code: None,
        };
        assert!(completed_receipt(&status, &status.job_id).is_ok());
        assert!(completed_receipt(&status, &Uuid::new_v4().to_string()).is_err());
        status.output = PathBuf::from("/exports/different");
        assert!(completed_receipt(&status, &status.job_id).is_err());
        status.output = status.receipt.as_ref().unwrap().output.clone();
        status.revision_sha256 = "d".repeat(64);
        assert!(completed_receipt(&status, &status.job_id).is_err());
        status.revision_sha256 = status.receipt.as_ref().unwrap().revision_sha256.clone();
        status.state = "running".into();
        assert!(completed_receipt(&status, &status.job_id).is_err());
    }
}
