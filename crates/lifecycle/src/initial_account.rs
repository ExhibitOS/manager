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
impl InitialAccountInput {
    pub(crate) fn validate(&self) -> Result<()> {
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

fn present(root: &Path, name: &str) -> Result<bool> {
    match fs::symlink_metadata(root.join(name)) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(failure("INITIAL_ACCOUNT_UNAVAILABLE")),
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
}
