//! Standard desktop-service activation. Native addressing and uncertain-reply
//! policy belong here; callers never interpolate application data into commands.
use lucent_domain::{AppId, ApplicationActionId};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use zbus::{connection::Builder, zvariant::Value};
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ActivationError {
    Unsupported,
    Failed,
}
fn endpoint(id: &AppId) -> Result<(String, String), ActivationError> {
    let name = id
        .as_str()
        .strip_suffix(".desktop")
        .ok_or(ActivationError::Unsupported)?;
    zbus::names::WellKnownName::try_from(name).map_err(|_| ActivationError::Unsupported)?;
    let path = format!("/{}", name.replace('.', "/").replace('-', "_"));
    zbus::zvariant::ObjectPath::try_from(path.as_str())
        .map_err(|_| ActivationError::Unsupported)?;
    Ok((name.into(), path))
}
fn error(error: zbus::Error) -> ActivationError {
    match error {
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.UnknownMethod"
                    | "org.freedesktop.DBus.Error.UnknownObject"
            ) =>
        {
            ActivationError::Unsupported
        }
        _ => ActivationError::Failed,
    }
}
pub(super) fn activate(id: &AppId, address: Option<&str>) -> Result<(), ActivationError> {
    activate_for(id, address, Duration::from_secs(5))
}
pub(super) fn activate_action(
    id: &AppId,
    action: &ApplicationActionId,
    address: Option<&str>,
) -> Result<(), ActivationError> {
    request_for(id, Some(action), address, Duration::from_secs(5))
}
fn activate_for(
    id: &AppId,
    address: Option<&str>,
    timeout: Duration,
) -> Result<(), ActivationError> {
    request_for(id, None, address, timeout)
}
fn request_for(
    id: &AppId,
    action: Option<&ApplicationActionId>,
    address: Option<&str>,
    timeout: Duration,
) -> Result<(), ActivationError> {
    let operation = action.map(Operation::Action).unwrap_or(Operation::Activate);
    request_context(id, operation, None, address, timeout)
}
pub(super) enum Operation<'a> {
    Activate,
    Action(&'a ApplicationActionId),
    Open(&'a [String]),
}
pub(super) fn request_context(
    id: &AppId,
    operation: Operation<'_>,
    activation_token: Option<&str>,
    address: Option<&str>,
    timeout: Duration,
) -> Result<(), ActivationError> {
    let (name, path) = endpoint(id)?;
    let deadline = Instant::now() + timeout;
    futures_lite::future::block_on(async {
        let builder = match address {
            Some(address) => Builder::address(address),
            None => Builder::session(),
        }
        .map_err(|_| ActivationError::Unsupported)?;
        let connection = bounded(builder.internal_executor(false).build(), deadline)
            .await
            .map_err(|_| ActivationError::Unsupported)?;
        let request = async {
            let mut platform_data: HashMap<&str, Value<'_>> = HashMap::new();
            if let Some(token) = activation_token {
                platform_data.insert("activation-token", Value::from(token));
            }
            let reply = if let Operation::Action(action) = operation {
                let parameters: Vec<Value<'_>> = vec![];
                connection
                    .call_method(
                        Some(name.as_str()),
                        path.as_str(),
                        Some("org.freedesktop.Application"),
                        "ActivateAction",
                        &(action.as_str(), parameters, platform_data),
                    )
                    .await
            } else if let Operation::Open(uris) = operation {
                connection
                    .call_method(
                        Some(name.as_str()),
                        path.as_str(),
                        Some("org.freedesktop.Application"),
                        "Open",
                        &(uris, platform_data),
                    )
                    .await
            } else {
                connection
                    .call_method(
                        Some(name.as_str()),
                        path.as_str(),
                        Some("org.freedesktop.Application"),
                        "Activate",
                        &(platform_data,),
                    )
                    .await
            }
            .map_err(error)?;
            reply
                .body()
                .deserialize::<()>()
                .map_err(|_| ActivationError::Failed)?;
            Ok(())
        };
        let result = futures_lite::future::or(
            async {
                futures_lite::future::or(request, async {
                    async_io::Timer::at(deadline).await;
                    Err(ActivationError::Failed)
                })
                .await
            },
            async {
                loop {
                    connection.executor().tick().await;
                }
            },
        )
        .await;
        let _ = futures_lite::future::or(connection.close(), async {
            async_io::Timer::after(Duration::from_millis(250)).await;
            Err(zbus::Error::Failure(
                "Activation connection close timed out".into(),
            ))
        })
        .await;
        result
    })
}
async fn bounded<T>(
    future: impl std::future::Future<Output = zbus::Result<T>>,
    deadline: Instant,
) -> zbus::Result<T> {
    futures_lite::future::or(future, async {
        async_io::Timer::at(deadline).await;
        Err(zbus::Error::Failure(
            "Activation connection deadline exceeded".into(),
        ))
    })
    .await
}
#[cfg(test)]
mod tests;
