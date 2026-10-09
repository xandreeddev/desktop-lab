//! Trusted-system adapters. Neither backend paints UI or implements password verification.
use greetd_ipc::{AuthMessageType, Request, Response};
use lucent_domain::*;
use pam_client::{Context, ConversationHandler, ErrorCode, Flag};
use std::{
    ffi::{CStr, CString},
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};
fn failed() -> DomainError {
    DomainError::Failed("Authentication failed. Try again.".into())
}
/// Current effective identity, obtained from the account database, never $USER.
pub fn current_identity() -> Result<String> {
    let mut buf = vec![0u8; 16384];
    let mut result = std::ptr::null_mut();
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    // SAFETY: buffers remain valid through getpwuid_r and the name is copied before dropping them.
    let code = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut result,
        )
    };
    if code != 0 || result.is_null() {
        return Err(DomainError::Unavailable(
            "Cannot resolve the session account".into(),
        ));
    }
    Ok(unsafe { CStr::from_ptr((*result).pw_name) }
        .to_string_lossy()
        .into_owned())
}
pub struct PamLocker;
struct Conversation<'a>(&'a mut dyn AuthConversation);
impl Conversation<'_> {
    fn ask(&mut self, kind: PromptKind, text: &CStr) -> std::result::Result<CString, ErrorCode> {
        let answer = self
            .0
            .prompt(AuthPrompt {
                kind,
                text: text.to_string_lossy().into_owned(),
            })
            .map_err(|_| ErrorCode::CONV_ERR)?;
        CString::new(answer.expose()).map_err(|_| ErrorCode::CONV_ERR)
    }
    fn inform(&mut self, kind: PromptKind, text: &CStr) {
        let _ = self.0.prompt(AuthPrompt {
            kind,
            text: text.to_string_lossy().into_owned(),
        });
    }
}
impl ConversationHandler for Conversation<'_> {
    fn prompt_echo_on(&mut self, p: &CStr) -> std::result::Result<CString, ErrorCode> {
        self.ask(PromptKind::Visible, p)
    }
    fn prompt_echo_off(&mut self, p: &CStr) -> std::result::Result<CString, ErrorCode> {
        self.ask(PromptKind::Secret, p)
    }
    fn text_info(&mut self, p: &CStr) {
        self.inform(PromptKind::Information, p)
    }
    fn error_msg(&mut self, p: &CStr) {
        self.inform(PromptKind::Error, p)
    }
}
impl AuthenticationPort for PamLocker {
    fn authenticate(&self, identity: &str, conversation: &mut dyn AuthConversation) -> Result<()> {
        if identity != current_identity()? {
            return Err(failed());
        }
        let mut context = Context::new(
            "omarchy-lock-password",
            Some(identity),
            Conversation(conversation),
        )
        .map_err(|_| failed())?;
        context
            .authenticate(Flag::DISALLOW_NULL_AUTHTOK)
            .map_err(|_| failed())?;
        context.acct_mgmt(Flag::NONE).map_err(|_| failed())?;
        Ok(())
    }
}
/// greetd owns PAM, privilege changes and the login session. This adapter only
/// speaks its documented protocol, with bounded reads and scrubbed JSON buffers.
pub struct Greetd {
    socket: std::path::PathBuf,
    command: Vec<String>,
    connection: std::sync::Mutex<Option<UnixStream>>,
}
impl Greetd {
    pub fn from_env(command: Vec<String>) -> Result<Self> {
        let socket = std::env::var_os("GREETD_SOCK")
            .ok_or_else(|| DomainError::Unavailable("greetd did not provide its socket".into()))?;
        Ok(Self {
            socket: socket.into(),
            command,
            connection: Default::default(),
        })
    }
    fn exchange(stream: &mut UnixStream, mut request: Request) -> Result<Response> {
        let bytes = Zeroizing::new(
            serde_json::to_vec(&request)
                .map_err(|_| DomainError::Invalid("Cannot encode greetd request".into()))?,
        );
        if let Request::PostAuthMessageResponse {
            response: Some(value),
        } = &mut request
        {
            value.zeroize();
        }
        stream
            .write_all(&(bytes.len() as u32).to_ne_bytes())
            .and_then(|_| stream.write_all(&bytes))
            .map_err(|e| DomainError::Unavailable(format!("greetd write: {e}")))?;
        let mut length = [0u8; 4];
        stream
            .read_exact(&mut length)
            .map_err(|e| DomainError::Unavailable(format!("greetd header: {e}")))?;
        let len = u32::from_ne_bytes(length) as usize;
        if len > 65536 {
            return Err(DomainError::Invalid(format!(
                "Oversized greetd response: {len}"
            )));
        }
        let mut bytes = Zeroizing::new(vec![0u8; len]);
        stream
            .read_exact(&mut bytes)
            .map_err(|e| DomainError::Unavailable(format!("greetd body: {e}")))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| DomainError::Unavailable(format!("Invalid greetd response: {error}")))
    }
}
impl AuthenticationPort for Greetd {
    fn authenticate(&self, identity: &str, conversation: &mut dyn AuthConversation) -> Result<()> {
        let mut connection = self.connection.lock().map_err(|_| failed())?;
        if connection.is_none() {
            let stream = UnixStream::connect(&self.socket)
                .map_err(|_| DomainError::Unavailable("Cannot connect to greetd".into()))?;
            stream
                .set_read_timeout(Some(Duration::from_secs(120)))
                .map_err(|_| failed())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| failed())?;
            *connection = Some(stream);
        }
        let stream = connection.as_mut().unwrap();
        let mut response = Self::exchange(
            stream,
            Request::CreateSession {
                username: identity.into(),
            },
        )?;
        loop {
            match response {
                Response::Success => break,
                Response::Error { .. } => {
                    let _ = Self::exchange(stream, Request::CancelSession);
                    return Err(failed());
                }
                Response::AuthMessage {
                    auth_message_type,
                    auth_message,
                } => {
                    let kind = match auth_message_type {
                        AuthMessageType::Visible => PromptKind::Visible,
                        AuthMessageType::Secret => PromptKind::Secret,
                        AuthMessageType::Info => PromptKind::Information,
                        AuthMessageType::Error => PromptKind::Error,
                    };
                    let answer = match conversation.prompt(AuthPrompt {
                        kind,
                        text: auth_message,
                    }) {
                        Ok(answer) => answer,
                        Err(e) => {
                            let _ = Self::exchange(stream, Request::CancelSession);
                            return Err(e);
                        }
                    };
                    let response_text = if matches!(kind, PromptKind::Secret | PromptKind::Visible)
                    {
                        Some(answer.expose().into())
                    } else {
                        None
                    };
                    response = Self::exchange(
                        stream,
                        Request::PostAuthMessageResponse {
                            response: response_text,
                        },
                    )?;
                }
            }
        }
        match Self::exchange(
            stream,
            Request::StartSession {
                cmd: self.command.clone(),
                env: vec![
                    "XDG_SESSION_TYPE=wayland".into(),
                    "XDG_CURRENT_DESKTOP=Hyprland".into(),
                ],
            },
        )? {
            Response::Success => Ok(()),
            _ => Err(failed()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use greetd_ipc::{ErrorType, codec::SyncCodec};
    struct Answer;
    impl AuthConversation for Answer {
        fn prompt(&mut self, p: AuthPrompt) -> Result<Secret> {
            assert_eq!(p.kind, PromptKind::Secret);
            Ok(Secret::new("fixture".into()))
        }
    }
    #[test]
    fn greetd_reuses_the_connection_for_retry_and_starts_only_after_success() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let backend = Greetd {
            socket: "unused".into(),
            command: vec!["fixture-session".into()],
            connection: std::sync::Mutex::new(Some(client)),
        };
        let worker = std::thread::spawn(move || {
            for succeeds in [false, true] {
                assert!(
                    matches!(Request::read_from(&mut server).unwrap(),Request::CreateSession {username} if username=="fixture-user")
                );
                Response::AuthMessage {
                    auth_message_type: AuthMessageType::Secret,
                    auth_message: "Password:".into(),
                }
                .write_to(&mut server)
                .unwrap();
                assert!(matches!(
                    Request::read_from(&mut server).unwrap(),
                    Request::PostAuthMessageResponse { response: Some(_) }
                ));
                if succeeds {
                    Response::Success.write_to(&mut server).unwrap();
                } else {
                    Response::Error {
                        error_type: ErrorType::AuthError,
                        description: "denied".into(),
                    }
                    .write_to(&mut server)
                    .unwrap();
                    assert!(matches!(
                        Request::read_from(&mut server).unwrap(),
                        Request::CancelSession
                    ));
                    Response::Success.write_to(&mut server).unwrap();
                }
            }
            assert!(
                matches!(Request::read_from(&mut server).unwrap(),Request::StartSession {cmd,..} if cmd==["fixture-session"])
            );
            Response::Success.write_to(&mut server).unwrap();
        });
        let rejected = backend.authenticate("fixture-user", &mut Answer);
        assert!(rejected.is_err(), "{rejected:?}");
        let accepted = backend.authenticate("fixture-user", &mut Answer);
        assert!(accepted.is_ok(), "{accepted:?}; first={rejected:?}");
        worker.join().unwrap();
    }
}
