use super::{
    catalog::{Catalog, Kind},
    native::*,
    *,
};
use std::collections::BTreeSet;
use zbus::zvariant::OwnedObjectPath;

pub(super) enum Plan {
    ControllerProperty(OwnedObjectPath, &'static str, bool),
    Discovery(OwnedObjectPath, bool),
    Device(OwnedObjectPath, &'static str),
    Trusted(OwnedObjectPath, bool),
    Forget(OwnedObjectPath, OwnedObjectPath),
}
pub(super) fn plan(
    command: BluetoothCommand,
    catalog: &Catalog,
    state: &BluetoothSnapshot,
) -> Result<Plan> {
    lucent_usecases::bluetooth::validate_bluetooth_command(state, &command)?;
    Ok(match command {
        BluetoothCommand::SetPowered { adapter, powered } => {
            Plan::ControllerProperty(catalog.resolve(adapter, Kind::Adapter)?, "Powered", powered)
        }
        BluetoothCommand::SetDiscoverable {
            adapter,
            discoverable,
        } => Plan::ControllerProperty(
            catalog.resolve(adapter, Kind::Adapter)?,
            "Discoverable",
            discoverable,
        ),
        BluetoothCommand::SetPairable { adapter, pairable } => Plan::ControllerProperty(
            catalog.resolve(adapter, Kind::Adapter)?,
            "Pairable",
            pairable,
        ),
        BluetoothCommand::SetDiscovery { adapter, active } => {
            Plan::Discovery(catalog.resolve(adapter, Kind::Adapter)?, active)
        }
        BluetoothCommand::Disconnect(id) => {
            Plan::Device(catalog.resolve(id, Kind::Device)?, "Disconnect")
        }
        BluetoothCommand::SetTrusted { device, trusted } => {
            Plan::Trusted(catalog.resolve(device, Kind::Device)?, trusted)
        }
        BluetoothCommand::Forget(id) => {
            let device = state
                .devices
                .iter()
                .find(|d| d.id == id)
                .expect("validated device");
            Plan::Forget(
                catalog.resolve(device.adapter, Kind::Adapter)?,
                catalog.resolve(id, Kind::Device)?,
            )
        }
    })
}
pub(super) async fn apply(
    transport: &Transport,
    plan: Plan,
    owned: &mut BTreeSet<String>,
) -> Result<()> {
    match plan {
        Plan::ControllerProperty(path, property, active) => {
            transport
                .set(path.as_str(), ADAPTER, property, active)
                .await?
        }
        Plan::Discovery(path, active) => {
            if owned.contains(path.as_str()) != active {
                transport
                    .call(
                        path.as_str(),
                        ADAPTER,
                        if active {
                            "StartDiscovery"
                        } else {
                            "StopDiscovery"
                        },
                        &(),
                    )
                    .await?;
                if active {
                    owned.insert(path.to_string());
                } else {
                    owned.remove(path.as_str());
                }
            }
        }
        Plan::Device(path, member) => {
            transport.call(path.as_str(), DEVICE, member, &()).await?;
        }
        Plan::Trusted(path, trusted) => {
            transport
                .set(path.as_str(), DEVICE, "Trusted", trusted)
                .await?
        }
        Plan::Forget(adapter, device) => {
            transport
                .call(adapter.as_str(), ADAPTER, "RemoveDevice", &(device,))
                .await?;
        }
    }
    Ok(())
}
