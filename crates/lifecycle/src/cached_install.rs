// SPDX-License-Identifier: Apache-2.0
//! Shares normal image provenance checks; cached-only admission never acquires images.
use super::*;

pub(super) fn admit_images(
    m: &BundleManifest,
    bundle: &Path,
    cached_images_only: bool,
    mut backend: impl FnMut(&[String], u64) -> Result<Vec<u8>>,
) -> Result<()> {
    for image in &m.images {
        if let Some(a) = &image.archive {
            let p = checked_path(&bundle, &a.path)?;
            let metadata = fs::metadata(&p).map_err(|_| err("BUNDLE_INVALID"))?;
            if !metadata.is_file()
                || a.bytes == 0
                || metadata.len() != a.bytes
                || a.bytes > 8 * 1024 * 1024 * 1024
                || !hash_valid(&a.sha256)
            {
                return Err(err("BUNDLE_INVALID"));
            }
            let mut f = File::open(&p).map_err(|_| err("BUNDLE_INVALID"))?;
            let mut h = Sha256::new();
            let mut b = [0; 65536];
            loop {
                let n = f.read(&mut b).map_err(|_| err("BUNDLE_INVALID"))?;
                if n == 0 {
                    break;
                }
                h.update(&b[..n]);
            }
            if format!("{:x}", h.finalize()) != a.sha256 {
                return Err(err("BUNDLE_CHANGED"));
            }
            if !cached_images_only {
                backend(
                    &[
                        "load".into(),
                        "--input".into(),
                        p.to_string_lossy().into_owned(),
                    ],
                    180,
                )?;
            }
        }
        if !cached_images_only && image.archive.is_none() {
            if image.reference.starts_with("sha256:") {
                return Err(err("BUNDLE_INVALID"));
            }
            backend(&["pull".into(), image.reference.clone()], 180)?;
        }
        let inspected: Value = serde_json::from_slice(&backend(
            &["image".into(), "inspect".into(), image.reference.clone()],
            15,
        )?)
        .map_err(|_| err("IMAGE_INTEGRITY"))?;
        if image.reference.starts_with("sha256:")
            && !inspected[0]["Id"]
                .as_str()
                .is_some_and(|id| same_local_image_id(id, &image.reference))
        {
            return Err(err("IMAGE_INTEGRITY"));
        }
        if image.reference.contains("@sha256:")
            && !inspected[0]["RepoDigests"].as_array().is_some_and(|v| {
                v.iter().any(|d| {
                    d.as_str()
                        .is_some_and(|value| same_registry_pin(value, &image.reference))
                })
            })
        {
            return Err(err("IMAGE_INTEGRITY"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(reference: String) -> BundleManifest {
        BundleManifest {
            explicit_local_network: None,
            schema_version: "1.0.0-draft.1".into(),
            bundle_id: "cached-fixture".into(),
            version: "0.1.0".into(),
            protocol_version: "1".into(),
            compose_sha256: "a".repeat(64),
            preferred_engine: Some("docker".into()),
            project_name: "exhibitos-cached-fixture".into(),
            services: vec!["platform".into()],
            images: vec![Image {
                reference,
                archive: None,
            }],
            ports: vec![13200],
            open_url: "http://127.0.0.1:13200".into(),
            readiness_url: "http://127.0.0.1:13200/api/v1/readiness".into(),
            minimum_free_bytes: 1,
        }
    }
    #[test]
    fn cached_install_observes_exact_identity_without_acquiring_or_falling_back() {
        let expected = format!("sha256:{}", "b".repeat(64));
        let m = manifest(expected.clone());
        let mut calls = Vec::new();
        admit_images(&m, Path::new("/unused"), true, |args, timeout| {
            calls.push((args.to_vec(), timeout));
            Ok(serde_json::to_vec(&serde_json::json!([{ "Id": expected }])).unwrap())
        })
        .unwrap();
        assert_eq!(
            calls,
            vec![(
                vec![
                    "image".into(),
                    "inspect".into(),
                    m.images[0].reference.clone()
                ],
                15
            )]
        );
        let mut attempts = 0;
        let failure = admit_images(&m, Path::new("/unused"), true, |args, _| {
            attempts += 1;
            assert_eq!(args[0], "image");
            Err(err("ENGINE_OPERATION_FAILED"))
        })
        .unwrap_err();
        assert_eq!(failure.code, "ENGINE_OPERATION_FAILED");
        assert_eq!(attempts, 1);
        assert_eq!(
            admit_images(&m, Path::new("/unused"), true, |_, _| Ok(
                serde_json::to_vec(
                    &serde_json::json!([{ "Id": format!("sha256:{}", "c".repeat(64)) }])
                )
                .unwrap()
            ))
            .unwrap_err()
            .code,
            "IMAGE_INTEGRITY"
        );
    }
    #[test]
    fn normal_registry_install_keeps_existing_pull_and_registry_pin_checks() {
        let reference = format!("ghcr.io/exhibitos/platform@sha256:{}", "b".repeat(64));
        let m = manifest(reference.clone());
        let mut calls = Vec::new();
        admit_images(&m, Path::new("/unused"), false, |args, _| {
            calls.push(args.to_vec());
            Ok(if args[0] == "pull" {
                Vec::new()
            } else {
                serde_json::to_vec(&serde_json::json!([{ "RepoDigests": [reference] }])).unwrap()
            })
        })
        .unwrap();
        assert_eq!(
            calls,
            vec![
                vec!["pull".into(), reference.clone()],
                vec!["image".into(), "inspect".into(), reference]
            ]
        );
        assert_eq!(
            admit_images(
                &manifest(format!("sha256:{}", "a".repeat(64))),
                Path::new("/unused"),
                false,
                |_, _| panic!(
                    "legacy local-image-without-archive must refuse before engine mutation"
                )
            )
            .unwrap_err()
            .code,
            "BUNDLE_INVALID"
        );
    }
    #[test]
    fn cached_install_still_checks_declared_archive_before_any_engine_access() {
        let root =
            std::env::temp_dir().join(format!("exhibitos-cached-archive-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let bytes = b"synthetic archive bytes";
        fs::write(root.join("image.tar"), bytes).unwrap();
        let reference = format!("sha256:{}", "b".repeat(64));
        let mut m = manifest(reference.clone());
        m.images[0].archive = Some(Archive {
            path: "image.tar".into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        });
        let mut calls = 0;
        admit_images(&m, &root, true, |args, _| {
            calls += 1;
            assert_eq!(args[0], "image");
            Ok(serde_json::to_vec(&serde_json::json!([{ "Id": reference }])).unwrap())
        })
        .unwrap();
        assert_eq!(calls, 1);
        fs::write(root.join("image.tar"), b"tampered archive bytes!").unwrap();
        assert_eq!(
            admit_images(&m, &root, true, |_, _| panic!(
                "archive admission must precede engine access"
            ))
            .unwrap_err()
            .code,
            "BUNDLE_CHANGED"
        );
        m.images[0].archive.as_mut().unwrap().bytes += 1;
        assert_eq!(
            admit_images(&m, &root, true, |_, _| panic!(
                "size admission must precede engine access"
            ))
            .unwrap_err()
            .code,
            "BUNDLE_INVALID"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn guarded_retry_refuses_normal_install_and_changed_targets_before_history_or_engine() {
        let root = std::env::temp_dir().join(format!("exhibitos-cached-retry-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root.clone()).unwrap();
        let mut job = Job {
            id: Uuid::new_v4().to_string(),
            cached_images_only: false,
            action: Action::Install,
            state: JobState::Failed,
            attempt: 1,
            progress: 0,
            created_at: now(),
            updated_at: now(),
            error_code: Some("BUNDLE_INVALID".into()),
            guidance: None,
        };
        write_json(&root, "jobs.json", &vec![job.clone()]).unwrap();
        let before = fs::read(root.join("jobs.json")).unwrap();
        assert_eq!(
            service
                .retry_existing_images(&job.id, 1, true)
                .unwrap_err()
                .code,
            "RETRY_IMAGE_POLICY_MISMATCH"
        );
        assert_eq!(fs::read(root.join("jobs.json")).unwrap(), before);
        job.cached_images_only = true;
        write_json(&root, "jobs.json", &vec![job.clone()]).unwrap();
        let before = fs::read(root.join("jobs.json")).unwrap();
        assert_eq!(
            service
                .retry_existing_images(&Uuid::new_v4().to_string(), 1, true)
                .unwrap_err()
                .code,
            "RETRY_TARGET_CHANGED"
        );
        assert_eq!(
            service
                .retry_existing_images(&job.id, 2, true)
                .unwrap_err()
                .code,
            "RETRY_TARGET_CHANGED"
        );
        assert_eq!(fs::read(root.join("jobs.json")).unwrap(), before);
        let retry = service.retry_existing_images(&job.id, 1, true).unwrap();
        assert_eq!(retry.id, job.id);
        assert_eq!(retry.attempt, 2);
        assert!(retry.cached_images_only);
        assert_eq!(retry.error_code.as_deref(), Some("BUNDLE_INVALID"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cached_policy_is_legacy_compatible_and_retry_retains_same_job_policy() {
        let root = std::env::temp_dir().join(format!("exhibitos-cached-policy-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root.clone()).unwrap();
        let mut job = Job {
            id: Uuid::new_v4().to_string(),
            cached_images_only: false,
            action: Action::Install,
            state: JobState::Failed,
            attempt: 1,
            progress: 0,
            created_at: now(),
            updated_at: now(),
            error_code: Some("BUNDLE_INVALID".into()),
            guidance: None,
        };
        let legacy = serde_json::to_value(&job).unwrap();
        assert!(legacy.get("cachedImagesOnly").is_none());
        assert!(
            !serde_json::from_value::<Job>(legacy)
                .unwrap()
                .cached_images_only
        );
        job.cached_images_only = true;
        write_json(&root, "jobs.json", &vec![job.clone()]).unwrap();
        let retry = service.execute(Action::Retry).unwrap();
        assert!(retry.cached_images_only);
        assert_eq!(retry.id, job.id);
        assert_eq!(retry.action, Action::Install);
        assert_eq!(retry.state, JobState::Failed);
        assert_eq!(retry.error_code.as_deref(), Some("BUNDLE_INVALID"));
        write_json(&root, "installed.json", &serde_json::json!({})).unwrap();
        let before = fs::read(root.join("jobs.json")).unwrap();
        assert_eq!(
            service.execute(Action::Retry).unwrap_err().code,
            "CACHED_INSTALL_RECONCILIATION_REQUIRED"
        );
        assert_eq!(fs::read(root.join("jobs.json")).unwrap(), before);
        assert_eq!(
            service.install_existing_images(false).unwrap_err().code,
            "INSTALL_ACK_REQUIRED"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
