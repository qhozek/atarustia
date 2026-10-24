use crate::config::Credentials;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use grammers_client::{Client, SignInError, client::PasswordToken};
use grammers_session::Session;
use serde::Serialize;
use std::{
    io::{self, Write},
    time::Duration,
};
use tokio::sync::{Mutex, Notify};
use zeroize::Zeroizing;

pub async fn prompt(label: &str, secret: bool) -> Result<String> {
    let label = label.to_owned();
    tokio::task::spawn_blocking(move || -> Result<String> {
        if secret {
            return Ok(rpassword::prompt_password(label)?);
        }
        print!("{label}");
        io::stdout().flush()?;
        let mut text = String::new();
        if io::stdin().read_line(&mut text)? == 0 {
            bail!("stdin closed; use web login or a terminal");
        }
        Ok(text.trim().to_owned())
    })
    .await?
}
pub fn qr_url(token: &[u8]) -> String {
    format!(
        "tg://login?token={}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token)
    )
}
pub async fn terminal(client: &Client, credentials: &Credentials) -> Result<()> {
    terminal_with_phone(client, credentials, None).await
}
pub async fn terminal_with_phone(
    client: &Client,
    credentials: &Credentials,
    phone: Option<&str>,
) -> Result<()> {
    terminal_login(client, credentials, phone, None, None).await
}
pub async fn terminal_login(
    client: &Client,
    credentials: &Credentials,
    phone: Option<&str>,
    session: Option<std::sync::Arc<crate::session::EncryptedSession>>,
    mut updates: Option<&mut tokio::sync::mpsc::Receiver<grammers_session::updates::UpdatesLike>>,
) -> Result<()> {
    if phone.is_none()
        && let Some(session) = session
    {
        let choice = prompt("Login: [1] phone, [2] QR [1]: ", false).await?;
        if choice == "2" {
            let login = Login::new(client.clone(), session, credentials.clone());
            login.start_qr().await?;
            let mut shown = String::new();
            loop {
                let view = login.view().await;
                if let Stage::Qr { url, .. } = &*login.stage.lock().await
                    && *url != shown
                {
                    let qr = qrcode::QrCode::new(url).context("could not render login QR")?;
                    println!(
                        "{}",
                        qr.render::<char>()
                            .quiet_zone(true)
                            .module_dimensions(2, 1)
                            .build()
                    );
                    println!("Telegram → Settings → Devices → Link Desktop Device");
                    shown = url.clone();
                }
                if view.stage == "password" {
                    let password = Zeroizing::new(prompt("Cloud password: ", true).await?);
                    if let Err(e) = login.password(&password).await {
                        println!("Password was not accepted: {e}");
                    }
                } else if view.stage == "done" {
                    return Ok(());
                }
                if let Some(receiver) = updates.as_deref_mut() {
                    tokio::select! {
                        update=receiver.recv()=>{if let Some(update)=update{login.observe(update).await?;}else{bail!("login connection closed");}},
                        _=tokio::time::sleep(Duration::from_secs(2))=>{login.poll_qr().await?;}
                    }
                } else {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    login.poll_qr().await?;
                }
            }
        }
    }
    let phone = if let Some(phone) = phone {
        phone.to_owned()
    } else {
        prompt("Phone (+country code): ", false).await?
    };
    let token = tokio::time::timeout(
        Duration::from_secs(30),
        client.request_login_code(&phone, &credentials.api_hash),
    )
    .await??;
    loop {
        let code = Zeroizing::new(prompt("Telegram code: ", true).await?);
        match client.sign_in(&token, &code).await {
            Ok(_) => return Ok(()),
            Err(SignInError::InvalidCode) => println!("Invalid code. Try again."),
            Err(SignInError::PasswordRequired(mut token)) => loop {
                println!("2FA hint: {}", token.hint().unwrap_or("—"));
                let password = Zeroizing::new(prompt("Cloud password: ", true).await?);
                match client.check_password(token, password.as_str()).await {
                    Ok(_) => return Ok(()),
                    Err(SignInError::InvalidPassword(next)) => {
                        token = next;
                        println!("Invalid password.");
                    }
                    Err(e) => return Err(e.into()),
                }
            },
            Err(e) => return Err(e.into()),
        }
    }
}

enum Stage {
    Phone,
    Code {
        phone: String,
        hash: String,
        length: Option<i32>,
        via: String,
    },
    Qr {
        url: String,
        expires: i32,
    },
    Password(PasswordToken),
    Done,
}
#[derive(Serialize)]
pub struct LoginView {
    pub stage: &'static str,
    pub hint: Option<String>,
    pub qr: Option<String>,
    pub phone: Option<String>,
    pub length: Option<i32>,
    pub via: Option<String>,
    pub name: Option<String>,
}
pub struct Login {
    client: Client,
    session: std::sync::Arc<crate::session::EncryptedSession>,
    credentials: Credentials,
    stage: Mutex<Stage>,
    pub ready: Notify,
    name: Mutex<Option<String>>,
}
impl Login {
    pub fn new(
        client: Client,
        session: std::sync::Arc<crate::session::EncryptedSession>,
        credentials: Credentials,
    ) -> Self {
        Self {
            client,
            session,
            credentials,
            stage: Mutex::new(Stage::Phone),
            ready: Notify::new(),
            name: Mutex::new(None),
        }
    }
    pub async fn view(&self) -> LoginView {
        let view = LoginView {
            stage: "phone",
            hint: None,
            qr: None,
            phone: None,
            length: None,
            via: None,
            name: self.name.lock().await.clone(),
        };
        match &*self.stage.lock().await {
            Stage::Phone => LoginView {
                stage: "phone",
                hint: None,
                qr: None,
                ..view
            },
            Stage::Code {
                phone, length, via, ..
            } => LoginView {
                stage: "code",
                hint: None,
                qr: None,
                phone: Some(format!("+{phone}")),
                length: *length,
                via: Some(via.clone()),
                ..view
            },
            Stage::Qr { url, .. } => LoginView {
                stage: "qr",
                hint: None,
                qr: qr_svg(url).ok(),
                ..view
            },
            Stage::Password(t) => LoginView {
                stage: "password",
                hint: t.hint().map(str::to_owned),
                qr: None,
                ..view
            },
            Stage::Done => LoginView {
                stage: "done",
                hint: None,
                qr: None,
                ..view
            },
        }
    }
    pub async fn phone(&self, phone: &str) -> Result<()> {
        let phone = phone
            .chars()
            .filter(|c| !matches!(c, ' ' | '(' | ')' | '-'))
            .collect::<String>();
        anyhow::ensure!(
            phone.len() <= 24
                && phone
                    .trim_start_matches('+')
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                && phone.len() >= 7,
            "invalid phone"
        );
        let mut stage = self
            .stage
            .try_lock()
            .context("another login request is running")?;
        anyhow::ensure!(!matches!(*stage, Stage::Done), "already signed in");
        let phone = phone.trim_start_matches('+').to_owned();
        let request = grammers_client::tl::functions::auth::SendCode {
            phone_number: phone.clone(),
            api_id: self.credentials.api_id,
            api_hash: self.credentials.api_hash.clone(),
            settings: grammers_client::tl::types::CodeSettings {
                allow_flashcall: false,
                current_number: false,
                allow_app_hash: false,
                allow_missed_call: false,
                allow_firebase: false,
                logout_tokens: None,
                token: None,
                app_sandbox: None,
                unknown_number: false,
            }
            .into(),
        };
        let result =
            tokio::time::timeout(Duration::from_secs(30), self.client.invoke(&request)).await?;
        let result = match result {
            Err(grammers_client::InvocationError::Rpc(error))
                if error.code == 303 && error.value.is_some() =>
            {
                self.session
                    .set_home_dc_id(error.value.unwrap() as i32)
                    .await?;
                tokio::time::timeout(Duration::from_secs(30), self.client.invoke(&request))
                    .await??
            }
            other => other?,
        };
        use grammers_client::tl::enums::auth::SentCode;
        match result {
            SentCode::Code(sent) => {
                let (length, via) = code_delivery(&sent.r#type);
                *stage = Stage::Code {
                    phone,
                    hash: sent.phone_code_hash,
                    length,
                    via,
                };
            }
            SentCode::PaymentRequired(_) => bail!("payment"),
            SentCode::Success(_) => {
                self.finish().await?;
                *stage = Stage::Done;
                self.ready.notify_one();
            }
        }
        Ok(())
    }
    pub async fn code(&self, code: &str) -> Result<()> {
        anyhow::ensure!(
            (4..=8).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit()),
            "invalid code"
        );
        let mut stage = self
            .stage
            .try_lock()
            .context("another login request is running")?;
        let Stage::Code {
            phone,
            hash,
            length,
            ..
        } = &*stage
        else {
            bail!("request a login code first");
        };
        anyhow::ensure!(
            length.is_none_or(|length| length as usize == code.len()),
            "code_invalid"
        );
        let request = grammers_client::tl::functions::auth::SignIn {
            phone_number: phone.clone(),
            phone_code_hash: hash.clone(),
            phone_code: Some(code.to_owned()),
            email_verification: None,
        };
        match tokio::time::timeout(Duration::from_secs(30), self.client.invoke(&request)).await? {
            Ok(grammers_client::tl::enums::auth::Authorization::Authorization(_)) => {
                self.finish().await?;
                *stage = Stage::Done;
                self.ready.notify_one();
            }
            Ok(grammers_client::tl::enums::auth::Authorization::SignUpRequired(_)) => {
                bail!("phone_unoccupied")
            }
            Err(grammers_client::InvocationError::Rpc(error))
                if error.name == "SESSION_PASSWORD_NEEDED" =>
            {
                let grammers_client::tl::enums::account::Password::Password(password) =
                    tokio::time::timeout(
                        Duration::from_secs(30),
                        self.client
                            .invoke(&grammers_client::tl::functions::account::GetPassword {}),
                    )
                    .await??;
                *stage = Stage::Password(PasswordToken::new(password));
            }
            Err(grammers_client::InvocationError::Rpc(error))
                if matches!(
                    error.name.as_str(),
                    "PHONE_CODE_EXPIRED" | "PHONE_CODE_HASH_EMPTY"
                ) =>
            {
                *stage = Stage::Phone;
                return Err(grammers_client::InvocationError::Rpc(error).into());
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    pub async fn password(&self, password: &str) -> Result<()> {
        let mut stage = self
            .stage
            .try_lock()
            .context("another login request is running")?;
        let previous = std::mem::replace(&mut *stage, Stage::Phone);
        let Stage::Password(token) = previous else {
            *stage = previous;
            bail!("2FA was not requested");
        };
        match tokio::time::timeout(
            Duration::from_secs(30),
            self.client.check_password(token, password),
        )
        .await?
        {
            Ok(_) => {
                self.finish().await?;
                *stage = Stage::Done;
                self.ready.notify_one();
            }
            Err(SignInError::InvalidPassword(token)) => {
                *stage = Stage::Password(token);
                bail!("invalid password");
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    pub async fn start_qr(&self) -> Result<()> {
        let mut stage = self
            .stage
            .try_lock()
            .context("another login request is running")?;
        anyhow::ensure!(!matches!(*stage, Stage::Done), "already signed in");
        self.qr_request(&mut stage).await
    }
    pub async fn cancel(&self) -> Result<()> {
        let mut stage = self
            .stage
            .try_lock()
            .context("another login request is running")?;
        anyhow::ensure!(!matches!(*stage, Stage::Done), "already signed in");
        *stage = Stage::Phone;
        Ok(())
    }
    async fn finish(&self) -> Result<()> {
        let user = tokio::time::timeout(Duration::from_secs(30), self.client.get_me()).await??;
        *self.name.lock().await = user.first_name().map(str::to_owned);
        self.session.flush()?;
        Ok(())
    }
    pub async fn poll_qr(&self) -> Result<()> {
        let Ok(mut stage) = self.stage.try_lock() else {
            return Ok(());
        };
        if !matches!(*stage, Stage::Qr { expires,.. } if expires as u64<=crate::security::now()+1) {
            return Ok(());
        }
        self.qr_request(&mut stage).await
    }
    pub async fn observe(&self, update: grammers_session::updates::UpdatesLike) -> Result<()> {
        use grammers_client::tl::enums::{Update, Updates};
        use grammers_session::updates::UpdatesLike;
        let login_changed = match update {
            UpdatesLike::Updates(Updates::Updates(u)) => {
                u.updates.iter().any(|u| matches!(u, Update::LoginToken))
            }
            UpdatesLike::Updates(Updates::Combined(u)) => {
                u.updates.iter().any(|u| matches!(u, Update::LoginToken))
            }
            UpdatesLike::Updates(Updates::UpdateShort(u)) => matches!(u.update, Update::LoginToken),
            _ => false,
        };
        if login_changed {
            let mut stage = self.stage.lock().await;
            if matches!(*stage, Stage::Qr { .. }) {
                self.qr_request(&mut stage).await?;
            }
        }
        Ok(())
    }
    async fn qr_request(&self, stage: &mut Stage) -> Result<()> {
        use grammers_client::tl;
        let request = tl::functions::auth::ExportLoginToken {
            api_id: self.credentials.api_id,
            api_hash: self.credentials.api_hash.clone(),
            except_ids: vec![],
        };
        let result =
            tokio::time::timeout(Duration::from_secs(30), self.client.invoke(&request)).await?;
        let mut token = match result {
            Ok(t) => t,
            Err(grammers_client::InvocationError::Rpc(e))
                if e.name == "SESSION_PASSWORD_NEEDED" =>
            {
                let tl::enums::account::Password::Password(p) = tokio::time::timeout(
                    Duration::from_secs(30),
                    self.client.invoke(&tl::functions::account::GetPassword {}),
                )
                .await??;
                *stage = Stage::Password(PasswordToken::new(p));
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        if let tl::enums::auth::LoginToken::MigrateTo(ref migration) = token {
            let migration = migration.clone();
            self.session.set_home_dc_id(migration.dc_id).await?;
            let migrated = tokio::time::timeout(
                Duration::from_secs(30),
                self.client.invoke_in_dc(
                    migration.dc_id,
                    &tl::functions::auth::ImportLoginToken {
                        token: migration.token,
                    },
                ),
            )
            .await?;
            token = match migrated {
                Ok(token) => token,
                Err(grammers_client::InvocationError::Rpc(e))
                    if e.name == "SESSION_PASSWORD_NEEDED" =>
                {
                    let tl::enums::account::Password::Password(p) = tokio::time::timeout(
                        Duration::from_secs(30),
                        self.client.invoke(&tl::functions::account::GetPassword {}),
                    )
                    .await??;
                    *stage = Stage::Password(PasswordToken::new(p));
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };
        }
        match token {
            tl::enums::auth::LoginToken::Token(t) => {
                let url = qr_url(&t.token);
                *stage = Stage::Qr {
                    url,
                    expires: t.expires,
                };
            }
            tl::enums::auth::LoginToken::Success(_) => {
                self.finish().await?;
                *stage = Stage::Done;
                self.ready.notify_one();
            }
            tl::enums::auth::LoginToken::MigrateTo(_) => bail!("repeated QR migration"),
        }
        Ok(())
    }
}
fn code_delivery(kind: &grammers_client::tl::enums::auth::SentCodeType) -> (Option<i32>, String) {
    use grammers_client::tl::enums::auth::SentCodeType as T;
    let (length, via) = match kind {
        T::App(v) => (Some(v.length), "app"),
        T::Sms(v) => (Some(v.length), "sms"),
        T::Call(v) => (Some(v.length), "call"),
        T::FlashCall(_) => (None, "flashcall"),
        T::MissedCall(v) => (Some(v.length), "missedcall"),
        T::FragmentSms(v) => (Some(v.length), "fragmentsms"),
        T::EmailCode(v) => (Some(v.length), "emailcode"),
        _ => (None, "other"),
    };
    (length, via.into())
}
/// Port of web/core.py::qr_svg: the frontend receives the same rounded SVG geometry.
fn qr_svg(value: &str) -> Result<String> {
    let code = qrcode::QrCode::with_error_correction_level(value, qrcode::EcLevel::M)?;
    let size = code.width();
    let eyes = [(0, 0), (0, size - 7), (size - 7, 0)];
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if code[(x, y)] == qrcode::Color::Dark
                && !eyes
                    .iter()
                    .any(|(ey, ex)| *ey <= y && y < ey + 7 && *ex <= x && x < ex + 7)
            {
                path.push_str(&format!(
                    "M{} {}a.42 .42 0 1 0 .84 0a.42 .42 0 1 0-.84 0z",
                    x as f64 + 0.08,
                    y as f64 + 0.5
                ));
            }
        }
    }
    for (y, x) in eyes {
        for (offset, size, radius) in [(0., 7., 2.2), (1., 5., 1.4), (2., 3., 0.9)] {
            let x = x as f64 + offset;
            let y = y as f64 + offset;
            let d = size - 2. * radius;
            path.push_str(&format!("M{} {y}h{d}a{radius} {radius} 0 0 1 {radius} {radius}v{d}a{radius} {radius} 0 0 1 -{radius} {radius}h{}a{radius} {radius} 0 0 1 -{radius} -{radius}v{}a{radius} {radius} 0 0 1 {radius} -{radius}z",x+radius,2.*radius-size,2.*radius-size));
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\"><path fill=\"currentColor\" fill-rule=\"evenodd\" d=\"{path}\"/></svg>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qr_token_uses_telegram_base64url() {
        assert_eq!(qr_url(&[251, 255]), "tg://login?token=-_8");
    }
    fn login() -> (tempfile::TempDir, crate::telegram::Connection, Login) {
        let dir = tempfile::tempdir().unwrap();
        let key = std::sync::Arc::new(crate::session::MasterKey::load(dir.path()).unwrap());
        let credentials = Credentials {
            api_id: 12345,
            api_hash: "0".repeat(32),
        };
        let connection =
            crate::telegram::Connection::open(dir.path(), "test", &credentials, key).unwrap();
        let login = Login::new(
            connection.client.clone(),
            connection.session.clone(),
            credentials,
        );
        (dir, connection, login)
    }
    #[tokio::test]
    async fn invalid_input_does_not_touch_network_or_change_stage() {
        let (_dir, connection, l) = login();
        for phone in ["", "hello", "+123", "+12345678bad"] {
            assert!(l.phone(phone).await.is_err());
        }
        assert!(l.code("12345").await.is_err());
        assert!(l.password("not-requested").await.is_err());
        assert_eq!(l.view().await.stage, "phone");
        connection.close().await.unwrap();
    }
    #[tokio::test]
    async fn phone_code_view_and_cancel_match_original_frontend_fields() {
        let (_dir, connection, login) = login();
        *login.stage.lock().await = Stage::Code {
            phone: "12345678901".into(),
            hash: "not-public".into(),
            length: Some(6),
            via: "app".into(),
        };
        let view = serde_json::to_value(login.view().await).unwrap();
        assert_eq!(view["phone"], "+12345678901");
        assert_eq!(view["length"], 6);
        assert_eq!(view["via"], "app");
        assert!(!view.to_string().contains("not-public"));
        login.cancel().await.unwrap();
        assert_eq!(login.view().await.stage, "phone");
        connection.close().await.unwrap();
    }
    #[test]
    fn qr_svg_has_original_rounded_geometry() {
        let svg = qr_svg(&format!("tg://login?token={}", "A".repeat(43))).unwrap();
        assert!(svg.contains("viewBox=\"0 0 33 33\""));
        assert!(svg.contains("a.42 .42"));
        assert!(svg.contains("fill-rule=\"evenodd\""));
    }
    #[tokio::test]
    async fn qr_state_contains_a_local_svg_and_not_a_password() {
        let (_dir, connection, l) = login();
        *l.stage.lock().await = Stage::Qr {
            url: "tg://login?token=test".into(),
            expires: crate::security::now() as i32 + 60,
        };
        let view = l.view().await;
        assert_eq!(view.stage, "qr");
        assert!(view.qr.unwrap().contains("<svg"));
        assert!(view.hint.is_none());
        l.observe(grammers_session::updates::UpdatesLike::ConnectionClosed)
            .await
            .unwrap();
        assert_eq!(l.view().await.stage, "qr");
        connection.close().await.unwrap();
    }
}
