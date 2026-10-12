# Bluetooth service consumer

This headless executable consumes `BluetoothPort` through the native `BlueZ`
adapter. It starts one cancellable observing worker and uses the port's command
queue. It loads no Wayland surface, renderer, desktop component or terminal
Bluetooth wizard.

From the repository root:

```sh
cargo run --manifest-path lucent/Cargo.toml --locked -p bluetooth-service -- --observe --seconds 10
```

Default observation prints counts only. `--stdio` opts into local device metadata
and newline-delimited JSON controls. `--bus-address` selects a private test bus;
production uses the system bus. See [the service guide](../../../docs/native-bluetooth.md)
for commands, ownership, deadlines, evidence scope and remaining pairing/UI work.
