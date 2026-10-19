use crate::config::Credentials;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use grammers_client::{
    Client, SignInError,
    client::{LoginToken, PasswordToken},
};
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
    Code(LoginToken),
    Qr { url: String, expires: i32 },
    Password(PasswordToken),
    Done,
}
#[derive(Serialize)]
pub struct LoginView {
    pub stage: &'static str,
    pub hint: Option<String>,
    pub qr: Option<String>,
}
pub struct Login {
    client: Client,
    session: std::sync::Arc<crate::session::EncryptedSession>,
    credentials: Credentials,
    stage: Mutex<Stage>,
    pub ready: Notify,
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
        }
    }
    pub async fn view(&self) -> LoginView {
        match &*self.stage.lock().await {
            Stage::Phone => LoginView {
                stage: "phone",
                hint: None,
                qr: None,
            },
            Stage::Code(_) => LoginView {
                stage: "code",
                hint: None,
                qr: None,
            },
            Stage::Qr { url, .. } => LoginView {
                stage: "qr",
                hint: None,
                qr: qrcode::QrCode::new(url).ok().map(|q| {
                    q.render::<qrcode::render::svg::Color>()
                        .min_dimensions(256, 256)
                        .build()
                }),
            },
            Stage::Password(t) => LoginView {
                stage: "password",
                hint: t.hint().map(str::to_owned),
                qr: None,
            },
            Stage::Done => LoginView {
                stage: "done",
                hint: None,
                qr: None,
            },
        }
    }
    pub async fn phone(&self, phone: &str) -> Result<()> {
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
        let token = tokio::time::timeout(
            Duration::from_secs(30),
            self.client
                .request_login_code(phone, &self.credentials.api_hash),
        )
        .await??;
        *stage = Stage::Code(token);
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
        let Stage::Code(token) = &*stage else {
            bail!("request a login code first");
        };
        match tokio::time::timeout(Duration::from_secs(30), self.client.sign_in(token, code))
            .await?
        {
            Ok(_) => {
                *stage = Stage::Done;
                self.ready.notify_one();
            }
            Err(SignInError::PasswordRequired(token)) => *stage = Stage::Password(token),
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
                *stage = Stage::Done;
                self.ready.notify_one();
            }
            tl::enums::auth::LoginToken::MigrateTo(_) => bail!("repeated QR migration"),
        }
        Ok(())
    }
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
