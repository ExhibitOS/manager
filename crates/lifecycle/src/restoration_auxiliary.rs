// SPDX-License-Identifier: Apache-2.0
//! Named image-declared auxiliary storage survives automatic helper removal.
use super::helper_reconciliation::{check_volume, declared_volumes};
use super::installation_backup::source_bytes;
use super::*;
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Auxiliary {
    format: u8,
    target_id: String,
    image: String,
    volume: String,
    destination: String,
    created_at: u64,
}
impl LifecycleService {
    pub(crate) fn prepare_restoration_auxiliary(
        &self,
        id: &str,
        image: &str,
        mut command: impl FnMut(&[String]) -> Result<Vec<u8>>,
    ) -> Result<Option<String>> {
        if !Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id)
            || !image.strip_prefix("sha256:").is_some_and(hash_valid)
        {
            return Err(err("OWNERSHIP_CONFLICT"));
        }
        let declared = declared_volumes(&mut command, &serde_json::json!({"Image":image}))?;
        if declared.is_empty() {
            return match fs::symlink_metadata(self.root.join(format!("restoration-aux-{id}.json")))
            {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(err("STATE_UNAVAILABLE")),
                Ok(_) => Err(err("STATE_INVALID")),
            };
        }
        let volume = format!("exhibitos-restore-aux-{id}");
        let name = format!("restoration-aux-{id}.json");
        let record_exists = match fs::symlink_metadata(self.root.join(&name)) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(err("STATE_UNAVAILABLE")),
        };
        let retained = if record_exists {
            let value: Auxiliary =
                serde_json::from_slice(&source_bytes(&self.root, &name, 4096, true)?)
                    .map_err(|_| err("STATE_INVALID"))?;
            if value.format != 1
                || value.target_id != id
                || value.image != image
                || value.volume != volume
                || value.destination != "/var/lib/postgresql"
                || value.created_at > now()
            {
                return Err(err("STATE_INVALID"));
            }
            Some(value)
        } else {
            None
        };
        let bytes = command(&[
            "volume".into(),
            "ls".into(),
            "--filter".into(),
            format!("name=^{volume}$"),
            "--format".into(),
            "{{.Name}}".into(),
        ])?;
        if bytes.len() > 1024 {
            return Err(err("ENGINE_OUTPUT_LIMIT"));
        }
        let names = String::from_utf8(bytes).map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
        let present = match names.trim() {
            "" => false,
            value if value == volume => true,
            _ => return Err(err("OWNERSHIP_CONFLICT")),
        };
        if !present {
            // Never fabricate a lost earlier volume; retain its journal for recovery.
            if retained.is_some() {
                return Err(err("RESTORE_RECOVERY_REQUIRED"));
            }
            let created = command(&[
                "volume".into(),
                "create".into(),
                "--driver".into(),
                "local".into(),
                "--label".into(),
                format!("com.exhibitos.restoration={id}"),
                volume.clone(),
            ])?;
            if String::from_utf8(created)
                .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?
                .trim()
                != volume
            {
                return Err(err("ENGINE_OUTPUT_INVALID"));
            }
        }
        check_volume(&mut command, &volume)?;
        if retained.is_none() {
            write_json(
                &self.root,
                &name,
                &Auxiliary {
                    format: 1,
                    target_id: id.into(),
                    image: image.into(),
                    volume: volume.clone(),
                    destination: "/var/lib/postgresql".into(),
                    created_at: now(),
                },
            )?;
        }
        Ok(Some(volume))
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn named_volume_is_owned_private_reused_and_missing_volume_never_recreated() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("restore-aux-test-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        let id = Uuid::new_v4().to_string();
        let image = format!("sha256:{}", "a".repeat(64));
        let volume = format!("exhibitos-restore-aux-{id}");
        let mut present = false;
        let mut creates = 0;
        let mut engine = |args: &[String]| -> Result<Vec<u8>> {
            if args[0] == "image" {
                return Ok(serde_json::to_vec(&serde_json::json!([{"Id":image,"Config":{"Volumes":{"/var/lib/postgresql":{}}}}])).unwrap());
            }
            match args[1].as_str(){
    "ls"=>Ok(if present{volume.as_bytes().to_vec()}else{Vec::new()}),
    "create"=>{creates+=1;present=true;Ok(volume.as_bytes().to_vec())},
    "inspect"=>Ok(serde_json::to_vec(&serde_json::json!([{"Name":volume,"Driver":"local","Options":null,"Labels":{"com.exhibitos.restoration":id}}])).unwrap()),
    _=>panic!(),
   }
        };
        assert_eq!(
            service
                .prepare_restoration_auxiliary(&id, &image, &mut engine)
                .unwrap(),
            Some(volume.clone())
        );
        let path = service.root.join(format!("restoration-aux-{id}.json"));
        let original = fs::read(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        service
            .prepare_restoration_auxiliary(&id, &image, &mut engine)
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(creates, 1);
        let mut created = false;
        let result=service.prepare_restoration_auxiliary(&id,&image,|args|{
   if args[0]=="image" {return Ok(serde_json::to_vec(&serde_json::json!([{"Id":image,"Config":{"Volumes":{"/var/lib/postgresql":{}}}}])).unwrap());}
   if args[1]=="create" {created=true;}Ok(Vec::new())
  });
        assert!(result.is_err());
        assert!(!created);
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    #[test]
    fn foreign_volume_and_alias_record_fail_before_helper_launch() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("restore-aux-test-{}", Uuid::new_v4()));
        let service = LifecycleService::new(root).unwrap();
        let id = Uuid::new_v4().to_string();
        let image = format!("sha256:{}", "a".repeat(64));
        let volume = format!("exhibitos-restore-aux-{id}");
        let mut created = false;
        let result=service.prepare_restoration_auxiliary(&id,&image,|args|{
   if args[0]=="image" {return Ok(serde_json::to_vec(&serde_json::json!([{"Id":image,"Config":{"Volumes":{"/var/lib/postgresql":{}}}}])).unwrap());}
   if args[1]=="create" {created=true;}
   if args[1]=="ls" {return Ok(volume.as_bytes().to_vec());}
   Ok(serde_json::to_vec(&serde_json::json!([{"Name":volume,"Driver":"local","Options":null,"Labels":{"com.exhibitos.restoration":"foreign"}}])).unwrap())
  });
        assert!(result.is_err());
        assert!(!created);
        let outside = service.root.join("retained");
        fs::write(&outside, b"retained candidate").unwrap();
        std::os::unix::fs::symlink(
            &outside,
            service.root.join(format!("restoration-aux-{id}.json")),
        )
        .unwrap();
        assert!(service.prepare_restoration_auxiliary(&id,&image,|_|Ok(serde_json::to_vec(&serde_json::json!([{"Id":image,"Config":{"Volumes":{"/var/lib/postgresql":{}}}}])).unwrap())).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"retained candidate");
        let link = service.root.join(format!("restoration-aux-{id}.json"));
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(service.root.join("missing-candidate"), &link).unwrap();
        assert!(service.prepare_restoration_auxiliary(&id,&image,|_|Ok(serde_json::to_vec(&serde_json::json!([{"Id":image,"Config":{"Volumes":{"/var/lib/postgresql":{}}}}])).unwrap())).is_err());
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    }
}
