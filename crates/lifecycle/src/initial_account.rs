// SPDX-License-Identifier: Apache-2.0
//! User-chosen first local account; never returns the stored password or rotates an existing account.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialAccountInput {
    pub subject: String,
    pub password: String,
}
impl Drop for InitialAccountInput {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.password).into_bytes();
        bytes.fill(0);
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialAccountContext {
    pub state: String,
    pub tenant_id: Option<String>,
    pub subject: Option<String>,
}
fn failure(code: &str) -> LifecycleError {
    let guidance = match code {
        "INITIAL_ACCOUNT_REQUIRED" => "처음 사용할 계정과 비밀번호를 설정한 뒤 설치를 계속하세요.",
        "INITIAL_ACCOUNT_INVALID" => {
            "계정은 영문·숫자로 시작해 영문·숫자·_.@-으로 3–128자, 비밀번호는 12–1024바이트로 입력하세요. 줄바꿈·제어 문자는 사용할 수 없습니다."
        }
        "INITIAL_ACCOUNT_EXISTS" => {
            "기존 계정은 변경하지 않았습니다. 처음 입력한 비밀번호로 Studio에 로그인하고 설치를 다시 이어가세요."
        }
        _ => {
            "기존 설치의 계정 설정을 확인할 수 없습니다. 설정과 데이터를 보존했습니다. 기존 로그인 또는 백업 복구 안내를 확인하세요."
        }
    };
    LifecycleError {
        code: code.into(),
        guidance: guidance.into(),
    }
}
/// Only new ADMIN_PASSWORD values use this literal dotenv subset. Old raw alphanumeric values remain unchanged.
pub(crate) fn encode_password(value: &str) -> String {
    let mut output = String::from("\"");
    for c in value.chars() {
        if matches!(c, '\\' | '"' | '$') {
            output.push('\\');
        }
        output.push(c);
    }
    output.push('"');
    output
}
pub(crate) fn decode_password(value: &str) -> Result<String> {
    if !value.starts_with('"') {
        return Ok(value.into());
    }
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or_else(|| failure("INITIAL_ACCOUNT_UNAVAILABLE"))?;
    let mut output = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(next @ ('\\' | '"' | '$')) => output.push(next),
                _ => return Err(failure("INITIAL_ACCOUNT_UNAVAILABLE")),
            }
        } else if c == '"' || c == '$' {
            return Err(failure("INITIAL_ACCOUNT_UNAVAILABLE"));
        } else {
            output.push(c);
        }
    }
    Ok(output)
}
fn present(root: &Path, name: &str) -> Result<bool> {
    match fs::symlink_metadata(root.join(name)) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(failure("INITIAL_ACCOUNT_UNAVAILABLE")),
    }
}
impl InitialAccountInput {
    fn validate(&self) -> Result<()> {
        if !(3..=128).contains(&self.subject.len())
            || !self
                .subject
                .bytes()
                .enumerate()
                .all(|(i, c)| c.is_ascii_alphanumeric() || (i > 0 && b"_.@-".contains(&c)))
            || !(12..=1024).contains(&self.password.len())
            || self.password.chars().any(char::is_control)
        {
            return Err(failure("INITIAL_ACCOUNT_INVALID"));
        }
        Ok(())
    }
}
impl LifecycleService {
    pub fn initial_account_context(&self) -> Result<InitialAccountContext> {
        if !present(&self.root, "runtime.env")? {
            if present(&self.root, "installed.json")? {
                return Err(failure("INITIAL_ACCOUNT_UNAVAILABLE"));
            }
            return Ok(InitialAccountContext {
                state: "required".into(),
                tenant_id: None,
                subject: None,
            });
        }
        #[cfg(unix)]
        {
            let account = publication_source::read(&self.root, String::new())
                .map_err(|_| failure("INITIAL_ACCOUNT_UNAVAILABLE"))?;
            Ok(InitialAccountContext {
                state: "existing".into(),
                tenant_id: Some(account.tenant_id.clone()),
                subject: Some(account.subject.clone()),
            })
        }
        #[cfg(not(unix))]
        {
            Err(failure("LOCAL_CMS_UNSUPPORTED"))
        }
    }
    /// The selected-installation controller remains pinned across this preparation and Install.
    pub fn prepare_initial_account(&self, input: Option<InitialAccountInput>) -> Result<()> {
        let _lock = self.lock()?;
        #[cfg(any(unix, windows))]
        signed_release::trust::lifecycle_writer::admit(&self.root, &Action::Install)?;
        let context = self.initial_account_context()?;
        if context.state == "existing" {
            return if input.is_some() {
                Err(failure("INITIAL_ACCOUNT_EXISTS"))
            } else {
                Ok(())
            };
        }
        let input = input.ok_or_else(|| failure("INITIAL_ACCOUNT_REQUIRED"))?;
        input.validate()?;
        let m = self.manifest()?;
        let db_password = format!(
            "{}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let mut bytes=format!("EXHIBITOS_PORT={}\nPOSTGRES_PASSWORD={}\nDATABASE_URL=postgresql://exhibitos:{}@database:5432/exhibitos\nADMIN_SUBJECT={}\nADMIN_PASSWORD={}\nTENANT_ID={}\n",m.ports[0],db_password,db_password,input.subject,encode_password(&input.password),Uuid::new_v4()).into_bytes();
        let result = write_private_new(&self.root, "runtime.env", &bytes, 8192)
            .map_err(|_| failure("INITIAL_ACCOUNT_UNAVAILABLE"));
        bytes.fill(0);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn service() -> LifecycleService {
        let service = LifecycleService::new(super::super::tests::root_for_detection()).unwrap();
        fs::create_dir(service.root.join("bundle")).unwrap();
        let compose = b"services: {}\n";
        fs::write(service.root.join("bundle/compose.yaml"), compose).unwrap();
        let mut manifest = super::super::tests::manifest_for_detection();
        manifest.compose_sha256 = digest(compose);
        fs::write(
            service.root.join("bundle/manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        service
    }
    fn input() -> InitialAccountInput {
        InitialAccountInput {
            subject: "chosen-owner".into(),
            password: "synthetic-chosen-password".into(),
        }
    }
    #[test]
    fn fresh_user_input_required_without_writing_credentials() {
        let s = service();
        assert_eq!(s.initial_account_context().unwrap().state, "required");
        assert_eq!(
            s.prepare_initial_account(None).unwrap_err().code,
            "INITIAL_ACCOUNT_REQUIRED"
        );
        assert!(!s.root.join("runtime.env").exists());
    }
    #[test]
    fn user_chosen_account_remains_private_and_immutable_after_partial_install() {
        let s = service();
        s.prepare_initial_account(Some(input())).unwrap();
        let before = fs::read(s.root.join("runtime.env")).unwrap();
        let context = serde_json::to_string(&s.initial_account_context().unwrap()).unwrap();
        assert!(context.contains("chosen-owner"));
        assert!(!context.contains("synthetic-chosen-password"));
        assert!(!context.contains("DATABASE_URL"));
        assert_eq!(
            s.prepare_initial_account(Some(input())).unwrap_err().code,
            "INITIAL_ACCOUNT_EXISTS"
        );
        s.prepare_initial_account(None).unwrap();
        assert_eq!(before, fs::read(s.root.join("runtime.env")).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(s.root.join("runtime.env"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn installed_without_environment_is_never_fresh() {
        let s = service();
        fs::write(s.root.join("installed.json"), b"{}").unwrap();
        assert_eq!(
            s.initial_account_context().err().unwrap().code,
            "INITIAL_ACCOUNT_UNAVAILABLE"
        );
        assert_eq!(
            s.prepare_initial_account(Some(input())).unwrap_err().code,
            "INITIAL_ACCOUNT_UNAVAILABLE"
        );
        assert!(!s.root.join("runtime.env").exists());
    }
    #[test]
    fn invalid_existing_environment_is_preserved() {
        let s = service();
        write_private_new(&s.root, "runtime.env", b"invalid", 8192).unwrap();
        assert_eq!(
            s.initial_account_context().err().unwrap().code,
            "INITIAL_ACCOUNT_UNAVAILABLE"
        );
        assert_eq!(
            s.prepare_initial_account(Some(input())).unwrap_err().code,
            "INITIAL_ACCOUNT_UNAVAILABLE"
        );
        assert_eq!(fs::read(s.root.join("runtime.env")).unwrap(), b"invalid");
    }
    #[test]
    fn byte_rules_and_environment_injection_are_validated_without_echoing_input() {
        let s = service();
        for (subject, password) in [
            ("ab", "synthetic-password"),
            ("bad\nsubject", "synthetic-password"),
            ("owner", "short"),
            ("owner", "password\nADMIN_SUBJECT=changed"),
            ("owner", "password\0nul"),
        ] {
            let result = s
                .prepare_initial_account(Some(InitialAccountInput {
                    subject: subject.into(),
                    password: password.into(),
                }))
                .unwrap_err();
            assert_eq!(result.code, "INITIAL_ACCOUNT_INVALID");
            assert!(!result.guidance.contains(password));
            assert!(!s.root.join("runtime.env").exists());
        }
        assert!(
            InitialAccountInput {
                subject: "owner".into(),
                password: "가".repeat(4)
            }
            .validate()
            .is_ok()
        );
        assert!(
            InitialAccountInput {
                subject: "owner".into(),
                password: "가".repeat(342)
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn literal_password_roundtrip_and_legacy_raw_compatibility() {
        for password in [
            "plain-synthetic-value",
            "synthetic $VAR ${NAME} # comment",
            "quote's \"literal\" and \\ ending\\",
            "한국어 비밀번호 long",
        ] {
            let encoded = encode_password(password);
            assert_eq!(decode_password(&encoded).unwrap(), password);
        }
        assert_eq!(
            decode_password("old-random-alnum0123456789").unwrap(),
            "old-random-alnum0123456789"
        );
        for encoded in [
            "\"unterminated",
            "\"bad\\nsequence\"",
            "\"unescaped$VALUE\"",
        ] {
            assert!(decode_password(encoded).is_err());
        }
    }
    #[test]
    #[ignore = "Explicit read-only installed Docker Compose config qualification; no engine actions"]
    fn installed_compose_literal_password_qualification() {
        let s = service();
        let compose = s.root.join("literal-compose.yaml");
        fs::write(
            &compose,
            "services:\n  check:\n    image: scratch\n    env_file: runtime.env\n",
        )
        .unwrap();
        for password in [
            "synthetic$VAR value#end",
            "synthetic${VAR}literal",
            "synthetic\\backslashend",
            "synthetic-trailing\\",
            "synthetic'quote",
            "synthetic\\'quote",
            "synthetic\\nsequence",
            "synthetic\"doublequote",
            "한국어유니코드문자",
        ] {
            let bytes = format!("ADMIN_PASSWORD={}\n", encode_password(password));
            fs::write(s.root.join("runtime.env"), bytes).unwrap();
            let output = std::process::Command::new("docker")
                .args(["compose", "--env-file"])
                .arg(s.root.join("runtime.env"))
                .arg("-f")
                .arg(&compose)
                .args(["config", "--format", "json"])
                .env_remove("ADMIN_PASSWORD")
                .output()
                .expect("Docker Compose executable required");
            assert!(
                output.status.success(),
                "Synthetic Compose config failed; raw output withheld"
            );
            let config: Value =
                serde_json::from_slice(&output.stdout).expect("Compose JSON expected");
            // Docker Compose config.go escapes every $ to $$ while serializing its resolved model.
            let actual = config["services"]["check"]["environment"]["ADMIN_PASSWORD"]
                .as_str()
                .expect("resolved environment required")
                .replace("$$", "$");
            assert!(
                actual == password,
                "Resolved synthetic password differs; values withheld"
            );
        }
    }
}
