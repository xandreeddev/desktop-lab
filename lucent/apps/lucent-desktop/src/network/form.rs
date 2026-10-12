//! The private editing state owns a zeroizing secret; views borrow only its count.
use lucent_api::InputEdit;
use lucent_domain::*;

#[derive(Clone)]
pub enum Target {
    Visible {
        access_point: NetworkObjectId,
        device: NetworkObjectId,
        ssid: NetworkSsid,
        security: WifiSecurity,
    },
    Hidden {
        device: NetworkObjectId,
    },
}
pub struct Form {
    pub generation: u64,
    pub target: Target,
    pub name: String,
    pub security: WifiSecurity,
    pub remember: bool,
    password: Secret,
}
impl Form {
    pub fn new(generation: u64, target: Target) -> Self {
        let (name, security) = match &target {
            Target::Visible { ssid, security, .. } => (ssid.display_name(), *security),
            Target::Hidden { .. } => (String::new(), WifiSecurity::WpaPersonal),
        };
        Self {
            generation,
            target,
            name,
            security,
            remember: true,
            password: Secret::default(),
        }
    }
    pub fn device(&self) -> NetworkObjectId {
        match self.target {
            Target::Visible { device, .. } | Target::Hidden { device } => device,
        }
    }
    pub fn valid(&self, s: Option<&NetworkSnapshot>) -> bool {
        let Some(s) = s else { return false };
        if !s.wifi_enabled
            || !s.wifi_hardware_enabled
            || !s
                .device(self.device())
                .is_some_and(|d| d.managed && d.kind == NetworkKind::Wifi)
        {
            return false;
        }
        match &self.target {
            Target::Visible {
                access_point,
                ssid,
                security,
                ..
            } => s.access_point(*access_point).is_some_and(|a| {
                a.ssid.as_ref() == Some(ssid)
                    && a.security == *security
                    && a.device == self.device()
            }),
            Target::Hidden { .. } => true,
        }
    }
    pub fn password_length(&self) -> usize {
        self.password.len()
    }
    pub fn password_edit(&mut self, edit: InputEdit) {
        match edit {
            InputEdit::Insert(text) => self.password.push(&text),
            InputEdit::Replace(text) => {
                self.password.clear();
                self.password.push(&text);
            }
            InputEdit::Backspace => self.password.pop(),
            InputEdit::Clear => self.password.clear(),
        }
    }
    pub fn set_security(&mut self, security: WifiSecurity) {
        if matches!(self.target, Target::Hidden { .. })
            && matches!(
                security,
                WifiSecurity::Open
                    | WifiSecurity::WpaPersonal
                    | WifiSecurity::Wpa3Personal
                    | WifiSecurity::EnhancedOpen
            )
        {
            self.password.clear();
            self.security = security;
        }
    }
    pub fn submit(&mut self) -> Result<NetworkCommand> {
        let target = match &self.target {
            Target::Visible { access_point, .. } => NetworkJoinTarget::AccessPoint(*access_point),
            Target::Hidden { device } => NetworkJoinTarget::Hidden {
                device: *device,
                ssid: NetworkSsid::new(self.name.as_bytes().to_vec())?,
            },
        };
        let password = if matches!(
            self.security,
            WifiSecurity::Open | WifiSecurity::EnhancedOpen
        ) {
            None
        } else {
            Some(std::mem::take(&mut self.password))
        };
        let credentials = WifiCredentials::new(self.security, password)?;
        Ok(NetworkCommand::Join {
            target,
            credentials,
            remember: self.remember,
        })
    }
}
