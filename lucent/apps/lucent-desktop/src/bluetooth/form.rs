use lucent_api::InputEdit;
use lucent_domain::*;
use zeroize::Zeroize;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pin,
    Passkey,
}
pub struct Form {
    pub request: BluetoothPairingId,
    pub kind: Kind,
    input: Secret,
}
impl Form {
    pub fn for_request(request: &BluetoothPairingRequest) -> Option<Self> {
        let kind = match request.prompt {
            BluetoothPairingPrompt::PinCode => Kind::Pin,
            BluetoothPairingPrompt::Passkey => Kind::Passkey,
            _ => return None,
        };
        Some(Self {
            request: request.id,
            kind,
            input: Secret::default(),
        })
    }
    pub fn length(&self) -> usize {
        self.input.len()
    }
    pub fn edit(&mut self, edit: InputEdit) {
        let replace = matches!(&edit, InputEdit::Replace(_));
        match edit {
            InputEdit::Insert(mut text) | InputEdit::Replace(mut text) => {
                // No full credential ever enters the scene or inspector. Bound
                // client storage separately from the domain's submit validation.
                let limit = if self.kind == Kind::Pin { 16 } else { 6 };
                if replace {
                    self.input.clear();
                }
                let valid = text.bytes().all(|b| {
                    if self.kind == Kind::Pin {
                        b.is_ascii_alphanumeric()
                    } else {
                        b.is_ascii_digit()
                    }
                });
                if valid && self.input.len() + text.len() <= limit {
                    self.input.push(&text);
                }
                text.zeroize();
            }
            InputEdit::Backspace => self.input.pop(),
            InputEdit::Clear => self.input.clear(),
        }
    }
    pub fn answer(&mut self) -> Result<BluetoothPairingAnswer> {
        let input = std::mem::take(&mut self.input);
        match self.kind {
            Kind::Pin => BluetoothPin::new(input).map(BluetoothPairingAnswer::PinCode),
            Kind::Passkey => BluetoothPasskeyInput::new(input).map(BluetoothPairingAnswer::Passkey),
        }
    }
}
