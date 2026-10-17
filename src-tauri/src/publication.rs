// SPDX-License-Identifier: Apache-2.0
//! Local CMS credentials and generated files never cross the webview boundary.
use exhibitos_lifecycle::{LifecycleError, LifecycleService};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static BUSY: AtomicBool = AtomicBool::new(false);
pub fn busy() -> bool {
    BUSY.load(Ordering::Acquire)
}
struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
fn enter() -> Result<Busy, LifecycleError> {
    if super::hosted::busy() || super::remote::busy() {
        return Err(fail("LOCAL_CMS_BUSY"));
    }
    BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| fail("LOCAL_CMS_BUSY"))?;
    Ok(Busy)
}
fn fail(code: &str) -> LifecycleError {
    LifecycleError{code:code.into(),guidance:"로컬 전시를 준비하지 못했습니다. 실행 상태와 Studio의 공개 상태를 확인하세요. 기존 파일은 보존했습니다.".into()}
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Publication {
    publication_id: String,
    exhibition_id: String,
    title: String,
    revision_sha256: String,
    published_at: String,
    status: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    items: Vec<Publication>,
}
struct CatalogPin {
    selection: String,
    origin: String,
    items: Vec<Publication>,
}
static CATALOG: Mutex<Option<CatalogPin>> = Mutex::new(None);
fn selected(
    pin: &CatalogPin,
    selection: &str,
    origin: &str,
    id: &str,
) -> Result<Publication, LifecycleError> {
    if pin.selection != selection || pin.origin != origin {
        return Err(fail("LOCAL_CMS_SELECTION_REQUIRED"));
    }
    pin.items
        .iter()
        .find(|p| p.publication_id == id)
        .cloned()
        .ok_or_else(|| fail("LOCAL_CMS_SELECTION_REQUIRED"))
}

fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
fn valid_catalog(c: &Catalog) -> bool {
    c.items.len() <= 100
        && c.items.iter().all(|p| {
            uuid(&p.publication_id)
                && uuid(&p.exhibition_id)
                && !p.title.is_empty()
                && p.title.chars().count() <= 256
                && !p.title.chars().any(char::is_control)
                && hex(&p.revision_sha256)
                && p.status == "published"
                && p.published_at.len() <= 64
        })
        && {
            let mut ids = std::collections::BTreeSet::new();
            c.items.iter().all(|p| ids.insert(&p.publication_id))
        }
}
fn verify_exporter(resources: &Path) -> Result<(), LifecycleError> {
    let expected = include_bytes!(concat!(env!("OUT_DIR"), "/publication-manifest.json"));
    let root = resources.join("publication-runtime/generated");
    let (manifest, pin) = super::remote::bounded(&root.join("manifest.json"), 4 * 1024 * 1024)?;
    if manifest.as_slice() != expected.as_slice() {
        return Err(fail("LOCAL_CMS_RUNTIME_CHANGED"));
    }
    pin.check()?;
    let value: serde_json::Value =
        serde_json::from_slice(&manifest).map_err(|_| fail("LOCAL_CMS_RUNTIME_CHANGED"))?;
    if value["format"] != "exhibitos-manager-publication-runtime-v1"
        || value["links"].as_object().is_none_or(|v| !v.is_empty())
    {
        return Err(fail("LOCAL_CMS_RUNTIME_CHANGED"));
    }
    let files = value["files"]
        .as_object()
        .ok_or_else(|| fail("LOCAL_CMS_RUNTIME_CHANGED"))?;
    if !files.contains_key("exporter.mjs") || !files.contains_key("viewer/static.html") {
        return Err(fail("LOCAL_CMS_RUNTIME_CHANGED"));
    }
    // Export only: sequential bounded reads avoid thousands of retained descriptors.
    for (name, record) in files {
        if name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.starts_with('.'))
            || name.starts_with('/')
            || name.contains('\\')
        {
            return Err(fail("LOCAL_CMS_RUNTIME_CHANGED"));
        }
        let size = record["bytes"]
            .as_u64()
            .filter(|n| *n <= 100 * 1024 * 1024)
            .ok_or_else(|| fail("LOCAL_CMS_RUNTIME_CHANGED"))?;
        let (bytes, file) = super::remote::bounded(&root.join(name), size)?;
        if bytes.len() as u64 != size
            || format!("{:x}", Sha256::digest(&bytes)) != record["sha256"].as_str().unwrap_or("")
        {
            return Err(fail("LOCAL_CMS_RUNTIME_CHANGED"));
        }
        file.check()?;
    }
    Ok(())
}
fn run(resources: &Path, mut input: Vec<u8>) -> Result<serde_json::Value, LifecycleError> {
    let pins = super::remote::verified_resources(resources)?;
    let mut command = Command::new(resources.join("remote-runtime/generated/node"));
    command
        .args(["--disable-proto=delete", "--no-warnings"])
        .arg(resources.join("scripts/publication-runner.mjs"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    for pin in &pins {
        pin.check()?;
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => {
            input.fill(0);
            return Err(fail("LOCAL_CMS_RUNTIME_UNAVAILABLE"));
        }
    };
    let written = child
        .stdin
        .take()
        .ok_or(())
        .and_then(|mut pipe| pipe.write_all(&input).map_err(|_| ()));
    input.fill(0);
    if written.is_err() {
        super::remote::stop(&mut child);
        return Err(fail("LOCAL_CMS_UNAVAILABLE"));
    }
    let out = child
        .stdout
        .take()
        .ok_or_else(|| fail("LOCAL_CMS_UNAVAILABLE"))?;
    let err = child
        .stderr
        .take()
        .ok_or_else(|| fail("LOCAL_CMS_UNAVAILABLE"))?;
    let (tx, rx) = mpsc::channel();
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(super::remote::reader(out, 262144));
    });
    std::thread::spawn(move || {
        let _ = etx.send(super::remote::reader(err, 4096));
    });
    let start = Instant::now();
    let mut output = None;
    let mut errors = None;
    let mut status = None;
    loop {
        if output.is_none() {
            output = rx.try_recv().ok();
        }
        if errors.is_none() {
            errors = erx.try_recv().ok();
        }
        if status.is_none() {
            status = match child.try_wait() {
                Ok(s) => s,
                Err(_) => {
                    super::remote::stop(&mut child);
                    return Err(fail("LOCAL_CMS_UNAVAILABLE"));
                }
            };
        }
        if output.as_ref().is_some_and(Result::is_err)
            || errors.as_ref().is_some_and(Result::is_err)
            || start.elapsed() > Duration::from_secs(180)
        {
            super::remote::stop(&mut child);
            return Err(fail("LOCAL_CMS_UNAVAILABLE"));
        }
        if status.is_some() && output.is_some() && errors.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for pin in &pins {
        pin.check()?;
    }
    let value: serde_json::Value = serde_json::from_slice(
        &output
            .unwrap()
            .map_err(|_| fail("LOCAL_CMS_RESPONSE_INVALID"))?,
    )
    .map_err(|_| fail("LOCAL_CMS_RESPONSE_INVALID"))?;
    if !status.unwrap().success() {
        let code = value["errorCode"].as_str().unwrap_or("");
        return Err(fail(
            if [
                "LOCAL_CMS_AUTH_REQUIRED",
                "LOCAL_CMS_PERMISSION_REQUIRED",
                "LOCAL_CMS_RESPONSE_INVALID",
                "LOCAL_CMS_UNAVAILABLE",
                "LOCAL_CMS_REQUEST_INVALID",
                "LOCAL_CMS_SOURCE_CHANGED",
                "LOCAL_CMS_EXPORT_FAILED",
            ]
            .contains(&code)
                && value.as_object().is_some_and(|m| m.len() == 1)
            {
                code
            } else {
                "LOCAL_CMS_UNAVAILABLE"
            },
        ));
    }
    Ok(value)
}
pub fn catalog(
    resources: PathBuf,
    service: &LifecycleService,
    selection: String,
) -> Result<Catalog, LifecycleError> {
    let _busy = enter()?;
    // Clear stale references even when the fresh authenticated lookup fails.
    *CATALOG.lock().map_err(|_| fail("LOCAL_CMS_UNAVAILABLE"))? = None;
    let credentials = service.local_publication_credentials()?;
    let password = std::str::from_utf8(credentials.password())
        .map_err(|_| fail("LOCAL_CMS_CREDENTIALS_INVALID"))?;
    let input=serde_json::to_vec(&serde_json::json!({"action":"catalog","origin":credentials.origin,"tenantId":credentials.tenant_id,"subject":credentials.subject,"password":password})).map_err(|_|fail("LOCAL_CMS_REQUEST_INVALID"))?;
    let value = run(&resources, input)?;
    let result: Catalog =
        serde_json::from_value(value).map_err(|_| fail("LOCAL_CMS_RESPONSE_INVALID"))?;
    if !valid_catalog(&result) {
        return Err(fail("LOCAL_CMS_RESPONSE_INVALID"));
    }
    *CATALOG.lock().map_err(|_| fail("LOCAL_CMS_UNAVAILABLE"))? = Some(CatalogPin {
        selection,
        origin: credentials.origin.clone(),
        items: result.items.clone(),
    });
    Ok(result)
}
pub fn export(
    resources: PathBuf,
    service: &LifecycleService,
    selection: &str,
    id: &str,
) -> Result<super::hosted::SourceInfo, LifecycleError> {
    let _busy = enter()?;
    let origin = service.open_url()?;
    let item = {
        let pin = CATALOG
            .lock()
            .map_err(|_| fail("LOCAL_CMS_SELECTION_REQUIRED"))?;
        let pin = pin
            .as_ref()
            .ok_or_else(|| fail("LOCAL_CMS_SELECTION_REQUIRED"))?;
        selected(pin, selection, &origin, id)?
    };
    verify_exporter(&resources)?;
    let output = service.local_publication_output()?;
    let path = output
        .to_str()
        .ok_or_else(|| fail("LOCAL_CMS_OUTPUT_UNAVAILABLE"))?;
    let value=run(&resources,serde_json::to_vec(&serde_json::json!({"action":"export","origin":origin,"publicationId":id,"expectedRevisionSha256":item.revision_sha256,"output":path})).map_err(|_|fail("LOCAL_CMS_REQUEST_INVALID"))?)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Receipt {
        publication_id: String,
        revision_sha256: String,
        manifest_sha256: String,
    }
    let receipt: Receipt =
        serde_json::from_value(value).map_err(|_| fail("LOCAL_CMS_RESPONSE_INVALID"))?;
    if receipt.publication_id != id
        || receipt.revision_sha256 != item.revision_sha256
        || !hex(&receipt.manifest_sha256)
    {
        return Err(fail("LOCAL_CMS_SOURCE_CHANGED"));
    }
    super::hosted::select_local_export(path, id, &item.revision_sha256, &receipt.manifest_sha256)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_exporter_matches_compiled_manifest_and_file_hashes() {
        verify_exporter(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    }
    #[test]
    fn closed_catalog_rejects_private_fields_duplicates_and_drafts() {
        let p = Publication {
            publication_id: "00000000-0000-4000-8000-000000000001".into(),
            exhibition_id: "00000000-0000-4000-8000-000000000002".into(),
            title: "전시".into(),
            revision_sha256: "a".repeat(64),
            published_at: "2026-10-17T00:00:00Z".into(),
            status: "published".into(),
        };
        assert!(valid_catalog(&Catalog {
            items: vec![p.clone()]
        }));
        assert!(!valid_catalog(&Catalog {
            items: vec![p.clone(), p.clone()]
        }));
        let pin = CatalogPin {
            selection: "one".into(),
            origin: "http://127.0.0.1:1234".into(),
            items: vec![p.clone()],
        };
        assert!(selected(&pin, "one", &pin.origin, &p.publication_id).is_ok());
        assert!(selected(&pin, "two", &pin.origin, &p.publication_id).is_err());
        assert!(selected(&pin, "one", "http://127.0.0.1:5678", &p.publication_id).is_err());
        assert!(selected(&pin, "one", &pin.origin, "unknown").is_err());
        let mut draft = p;
        draft.status = "draft".into();
        assert!(!valid_catalog(&Catalog { items: vec![draft] }));
        assert!(serde_json::from_str::<Catalog>(r#"{"items":[],"password":"secret"}"#).is_err());
    }
}
