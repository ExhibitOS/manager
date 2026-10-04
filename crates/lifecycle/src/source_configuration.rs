// SPDX-License-Identifier: Apache-2.0
//! Current native freeze key observation; not complete configuration inventory.
use crate::*;
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeConfigurationProof {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeConfigurationReceipt {
    pub source_instance: String,
    pub target_instance: String,
    pub backup_id: String,
    pub authenticated_manifest_sha256: String,
    pub configuration_volume: String,
    pub maintenance_image: String,
    pub file: NativeConfigurationProof,
    pub observed_at: u64,
}
pub(super) fn expected(manifest: &[u8]) -> Result<NativeConfigurationProof> {
    let v: Value =
        serde_json::from_slice(manifest).map_err(|_| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?;
    let files = v["files"]
        .as_array()
        .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?;
    let selected: Vec<_> = files
        .iter()
        .filter(|f| f["name"] == "freeze-signing-key.json")
        .collect();
    if selected.len() != 1 || selected[0]["role"] != "configuration" {
        return Err(err("UPDATE_SOURCE_CONFIGURATION_INVALID"));
    }
    let f = selected[0];
    Ok(NativeConfigurationProof {
        name: "freeze-signing-key.json".into(),
        bytes: f["bytes"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 1048576)
            .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?,
        sha256: f["sha256"]
            .as_str()
            .filter(|h| hash_valid(h))
            .ok_or_else(|| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?
            .into(),
    })
}
const READER: &str = r#"
import {open,lstat,realpath,readdir,statfs} from 'node:fs/promises';
import {constants} from 'node:fs';
import {createHash} from 'node:crypto';
const path='/configuration/freeze-signing-key.json';
if(JSON.stringify((await readdir('/configuration')).sort())!==JSON.stringify(['freeze-signing-key.json']))throw Error('SOURCE_CONFIGURATION_SCOPE');
async function observe(){
 if(await realpath(path)!==path||(await statfs(path,{bigint:true})).type===0x65735546n)throw Error('SOURCE_CONFIGURATION_FILESYSTEM_UNVERIFIED');
 const f=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
 let bytes;
 try{
  const before=await f.stat({bigint:true});
  if(!before.isFile()||before.nlink!==1n||(before.mode&0o7777n)!==0o600n||before.uid!==1000n||before.size<1n||before.size>1048576n)throw Error('SOURCE_CONFIGURATION_MODE');
  bytes=await f.readFile();const after=await f.stat({bigint:true}),current=await lstat(path,{bigint:true});
  if(await realpath(path)!==path||(await statfs(path,{bigint:true})).type===0x65735546n||!current.isFile()||BigInt(bytes.length)!==before.size||['dev','ino','size','mode','uid','gid','nlink','mtimeNs','ctimeNs'].some(k=>before[k]!==after[k]||before[k]!==current[k]))throw Error('SOURCE_CONFIGURATION_CHANGED');
  return {name:'freeze-signing-key.json',bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')};
 }finally{if(bytes)bytes.fill(0);await f.close();}
}
const first=await observe(),second=await observe();
if(JSON.stringify(first)!==JSON.stringify(second)||JSON.stringify((await readdir('/configuration')).sort())!==JSON.stringify(['freeze-signing-key.json']))throw Error('SOURCE_CONFIGURATION_CHANGED');
console.log(JSON.stringify(first));
"#;
pub(super) fn observe(
    image: &str,
    volume: &str,
    expected: &NativeConfigurationProof,
) -> Result<NativeConfigurationProof> {
    if !image.strip_prefix("sha256:").is_some_and(hash_valid)
        || volume.is_empty()
        || !volume
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err(err("BACKUP_IMAGE_INVALID"));
    }
    if crate::backup_creation::local_image("docker", image)? != image {
        return Err(err("IMAGE_INTEGRITY"));
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let name = format!("exhibitos-source-config-{nonce}");
    let label = format!("com.exhibitos.source.configuration={nonce}");
    let args = vec![
        "create".into(),
        "--pull".into(),
        "never".into(),
        "--name".into(),
        name.clone(),
        "--label".into(),
        label,
        "--network".into(),
        "none".into(),
        "--user".into(),
        "1000:1000".into(),
        "--read-only".into(),
        "--cap-drop".into(),
        "ALL".into(),
        "--security-opt".into(),
        "no-new-privileges:true".into(),
        "--pids-limit".into(),
        "32".into(),
        "--memory".into(),
        "256m".into(),
        "--tmpfs".into(),
        "/var/lib/postgresql:rw,nosuid,nodev,size=1m".into(),
        "--mount".into(),
        format!("type=volume,source={volume},target=/configuration,readonly"),
        "--entrypoint".into(),
        "node".into(),
        image.into(),
        "--input-type=module".into(),
        "-e".into(),
        READER.into(),
    ];
    // Retain uncertain creation; a missing CLI result is not evidence of no helper.
    let created = String::from_utf8(run("docker", &args, None, 30)?)
        .map_err(|_| err("ENGINE_OUTPUT_INVALID"))?;
    let id = created.trim();
    if !hash_valid(id) {
        return Err(err("ENGINE_OUTPUT_INVALID"));
    }
    let output = run(
        "docker",
        &["start".into(), "--attach".into(), id.into()],
        None,
        60,
    );
    // A timed-out attach must not leave an unobserved child running.
    let helper = crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
    if helper["Id"] != id
        || helper["Name"] != format!("/{name}")
        || helper["Config"]["Labels"]["com.exhibitos.source.configuration"] != nonce
    {
        return Err(err("OWNERSHIP_CONFLICT"));
    }
    if helper["State"]["Running"] != false {
        run(
            "docker",
            &["stop".into(), "--time".into(), "5".into(), id.into()],
            None,
            15,
        )?;
    }
    let stopped = crate::backup_creation::inspected("docker", &["inspect".into(), id.into()])?;
    if stopped["State"]["Running"] != false || stopped["State"]["Restarting"] != false {
        return Err(err("CANCEL_UNCERTAIN"));
    }
    run("docker", &["rm".into(), id.into()], None, 15)?; // no force / volume deletion
    let actual: NativeConfigurationProof =
        serde_json::from_slice(&output?).map_err(|_| err("UPDATE_SOURCE_CONFIGURATION_INVALID"))?;
    if actual != *expected {
        return Err(err("UPDATE_SOURCE_CONFIGURATION_MISMATCH"));
    }
    Ok(actual)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expected_key_record_must_be_unique_private_configuration_with_bounded_hash() {
        let record = serde_json::json!({"name":"freeze-signing-key.json","role":"configuration","bytes":3,"sha256":"a".repeat(64)});
        let manifest =
            |files: Vec<Value>| serde_json::to_vec(&serde_json::json!({"files":files})).unwrap();
        assert!(expected(&manifest(vec![record.clone()])).is_ok());
        assert!(expected(&manifest(vec![])).is_err());
        assert!(expected(&manifest(vec![record.clone(), record.clone()])).is_err());
        for (field, value) in [
            ("role", Value::from("blob")),
            ("bytes", Value::from(0)),
            ("bytes", Value::from(1048577)),
            ("sha256", Value::from("bad")),
        ] {
            let mut bad = record.clone();
            bad[field] = value;
            assert!(expected(&manifest(vec![bad])).is_err());
        }
    }
    #[test]
    #[ignore = "requires the existing maintenance image and local Docker"]
    fn actual_native_configuration_content_and_metadata() {
        let volume = format!("exhibitos-native-config-test-{}", uuid::Uuid::new_v4());
        let image = "sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06";
        let call = |args: &[String]| run("docker", args, None, 30).unwrap();
        call(&[
            "volume".into(),
            "create".into(),
            "--label".into(),
            "com.exhibitos.synthetic.configuration=true".into(),
            volume.clone(),
        ]);
        let mutate = |script: &str| {
            call(&[
                "run".into(),
                "--rm".into(),
                "--pull".into(),
                "never".into(),
                "--network".into(),
                "none".into(),
                "--user".into(),
                "0:0".into(),
                "--tmpfs".into(),
                "/var/lib/postgresql:rw,nosuid,nodev,size=1m".into(),
                "--mount".into(),
                format!("type=volume,source={volume},target=/configuration"),
                "--entrypoint".into(),
                "node".into(),
                image.into(),
                "--input-type=module".into(),
                "-e".into(),
                format!(
                    "import {{writeFile,chmod,chown,link,unlink}} from 'node:fs/promises';const p='/configuration/freeze-signing-key.json';{script}"
                ),
            ]);
        };
        mutate(
            "await writeFile(p,'synthetic private setting',{mode:0o600});await chown(p,1000,1000)",
        );
        let expected = NativeConfigurationProof {
            name: "freeze-signing-key.json".into(),
            bytes: b"synthetic private setting".len() as u64,
            sha256: digest(b"synthetic private setting"),
        };
        assert_eq!(observe(image, &volume, &expected).unwrap(), expected);
        mutate("await writeFile(p,'changed private setting');");
        assert_eq!(
            observe(image, &volume, &expected).unwrap_err().code,
            "UPDATE_SOURCE_CONFIGURATION_MISMATCH"
        );
        mutate("await writeFile(p,'synthetic private setting');await chmod(p,0o644)");
        assert!(observe(image, &volume, &expected).is_err());
        mutate("await chmod(p,0o600);await link(p,'/configuration/alias')");
        assert!(observe(image, &volume, &expected).is_err());
        mutate("await unlink('/configuration/alias');await chown(p,0,0)");
        assert!(observe(image, &volume, &expected).is_err());
        mutate("await chown(p,1000,1000);await writeFile('/configuration/extra','synthetic')");
        assert!(observe(image, &volume, &expected).is_err());
        mutate("await unlink('/configuration/extra')");
        assert_eq!(observe(image, &volume, &expected).unwrap(), expected);
        println!("Retained private synthetic configuration volume {volume}");
    }
}
