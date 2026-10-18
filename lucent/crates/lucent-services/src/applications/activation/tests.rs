use super::*;
#[test]
fn activation_endpoint_derives_literal_native_names_and_paths() {
    assert_eq!(
        endpoint(&AppId::new("org.example.Some-App.desktop").unwrap()).unwrap(),
        (
            "org.example.Some-App".into(),
            "/org/example/Some_App".into()
        )
    );
    assert_eq!(
        endpoint(&AppId::new("A label.desktop").unwrap()),
        Err(ActivationError::Unsupported)
    );
}

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().into(),
        }
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Application {
    count: Arc<Mutex<usize>>,
    deny: bool,
    delay: Duration,
}
#[zbus::interface(name = "org.freedesktop.Application")]
impl Application {
    async fn activate(
        &self,
        data: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        assert!(data.is_empty());
        *self.count.lock().unwrap() += 1;
        if self.delay > Duration::ZERO {
            async_io::Timer::after(self.delay).await;
        }
        if self.deny {
            Err(zbus::fdo::Error::AccessDenied(
                "private fixture refusal".into(),
            ))
        } else {
            Ok(())
        }
    }
}
#[test]
fn application_activation_native_success_unavailable_denial_and_uncertain_timeout() {
    for (deny, delay, expected) in [
        (false, Duration::ZERO, Ok(())),
        (true, Duration::ZERO, Err(ActivationError::Failed)),
        (
            false,
            Duration::from_millis(300),
            Err(ActivationError::Failed),
        ),
    ] {
        let bus = Bus::new();
        let count = Arc::new(Mutex::new(0));
        let connection = futures_lite::future::block_on(
            Builder::address(bus.address.as_str())
                .unwrap()
                .name("org.example.Fixture")
                .unwrap()
                .serve_at(
                    "/org/example/Fixture",
                    Application {
                        count: count.clone(),
                        deny,
                        delay,
                    },
                )
                .unwrap()
                .build(),
        )
        .unwrap();
        let id = AppId::new("org.example.Fixture.desktop").unwrap();
        assert_eq!(
            activate_for(&id, Some(&bus.address), Duration::from_millis(200)),
            expected
        );
        assert_eq!(*count.lock().unwrap(), 1);
        assert_eq!(
            activate(
                &AppId::new("org.example.Missing.desktop").unwrap(),
                Some(&bus.address)
            ),
            Err(ActivationError::Unsupported)
        );
        drop(connection);
    }
}

struct ActionApplication {
    calls: Arc<Mutex<Vec<String>>>,
    deny: bool,
    delay: Duration,
}
#[zbus::interface(name = "org.freedesktop.Application")]
impl ActionApplication {
    async fn activate_action(
        &self,
        action: String,
        parameters: Vec<zbus::zvariant::OwnedValue>,
        data: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        assert!(parameters.is_empty());
        // No ambient startup/activation token is forwarded as platform data.
        assert!(data.is_empty());
        self.calls.lock().unwrap().push(action);
        if self.delay > Duration::ZERO {
            async_io::Timer::after(self.delay).await;
        }
        if self.deny {
            Err(zbus::fdo::Error::AccessDenied(
                "private fixture refusal".into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[test]
fn action_activation_captures_exact_case_and_distinguishes_denial_timeout_and_absence() {
    for (deny, delay, expected) in [
        (false, Duration::ZERO, Ok(())),
        (true, Duration::ZERO, Err(ActivationError::Failed)),
        (
            false,
            Duration::from_millis(300),
            Err(ActivationError::Failed),
        ),
    ] {
        let bus = Bus::new();
        let calls = Arc::new(Mutex::new(vec![]));
        let connection = futures_lite::future::block_on(
            Builder::address(bus.address.as_str())
                .unwrap()
                .name("org.example.Fixture")
                .unwrap()
                .serve_at(
                    "/org/example/Fixture",
                    ActionApplication {
                        calls: calls.clone(),
                        deny,
                        delay,
                    },
                )
                .unwrap()
                .build(),
        )
        .unwrap();
        let id = AppId::new("org.example.Fixture.desktop").unwrap();
        let action = ApplicationActionId::new("New-Window").unwrap();
        assert_eq!(
            request_for(
                &id,
                Some(&action),
                Some(&bus.address),
                Duration::from_millis(200)
            ),
            expected
        );
        assert_eq!(*calls.lock().unwrap(), ["New-Window"]);
        assert_eq!(
            activate_action(
                &AppId::new("org.example.Missing.desktop").unwrap(),
                &action,
                Some(&bus.address)
            ),
            Err(ActivationError::Unsupported)
        );
        drop(connection);
    }
}

#[test]
fn primary_activate_method_does_not_substitute_for_an_unsupported_action() {
    let bus = Bus::new();
    let count = Arc::new(Mutex::new(0));
    let connection = futures_lite::future::block_on(
        Builder::address(bus.address.as_str())
            .unwrap()
            .name("org.example.Fixture")
            .unwrap()
            .serve_at(
                "/org/example/Fixture",
                Application {
                    count: count.clone(),
                    deny: false,
                    delay: Duration::ZERO,
                },
            )
            .unwrap()
            .build(),
    )
    .unwrap();
    assert_eq!(
        activate_action(
            &AppId::new("org.example.Fixture.desktop").unwrap(),
            &ApplicationActionId::new("New").unwrap(),
            Some(&bus.address)
        ),
        Err(ActivationError::Unsupported)
    );
    assert_eq!(*count.lock().unwrap(), 0);
    drop(connection);
}

struct WrongReply;
#[zbus::interface(name = "org.freedesktop.Application")]
impl WrongReply {
    async fn activate_action(
        &self,
        action: String,
        parameters: Vec<zbus::zvariant::OwnedValue>,
        data: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> u32 {
        assert_eq!(action, "New");
        assert!(parameters.is_empty() && data.is_empty());
        1
    }
}

#[test]
fn malformed_action_acknowledgement_is_uncertain_not_unsupported() {
    let bus = Bus::new();
    let connection = futures_lite::future::block_on(
        Builder::address(bus.address.as_str())
            .unwrap()
            .name("org.example.Fixture")
            .unwrap()
            .serve_at("/org/example/Fixture", WrongReply)
            .unwrap()
            .build(),
    )
    .unwrap();
    assert_eq!(
        activate_action(
            &AppId::new("org.example.Fixture.desktop").unwrap(),
            &ApplicationActionId::new("New").unwrap(),
            Some(&bus.address)
        ),
        Err(ActivationError::Failed)
    );
    drop(connection);
}
