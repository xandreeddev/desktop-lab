# Audio service observer

This independent client injects `PulseAudio` behind `Arc<dyn AudioPort>` and
consumes its snapshot subscription. It uses the running user's audio service,
including PipeWire's PulseAudio compatibility service. It has no stdin/control
mode, creates no playback/recording streams and changes no device or volume.

```sh
cargo run --manifest-path lucent/Cargo.toml --locked --offline --release \
  -p lucent-services --example audio-observer -- --observe --seconds 15
```

No arguments observes for 10 seconds. Durations must be 1–600 seconds. The
adapter drives native events; the consumer's deadline cancels observation and
the adapter releases its subscription/connection before `watch` returns. The
deadline is cooperative, not a hard process timeout: native calls and blocking
stdout can delay it. A parent process should impose its own execution timeout
when a strict bound is required.

Consumer tests use an injected `AudioPort` and run as part of workspace tests.
Run them separately with:

```sh
cargo test --manifest-path lucent/Cargo.toml --locked --offline \
  -p lucent-services --example audio-observer
```

Each JSON line reports ready state, unavailable state, or completion. Ready
snapshots contain output/input/stream counts and the default devices' observed
volume ratios and mute state. `inputs` includes monitor inputs, also counted
separately by `monitor_inputs`. Volume 1 is the normal maximum; amplification
above 1 remains visible. A missing default is `null`, rather than invented zero
volume. No device name, key/address, application metadata or native error body
is printed. Completion records observed ready/unavailable counts and whether
`watch` returned after cancellation. Exit succeeds only after cancellation and
at least one ready snapshot; unavailable events remain explicit even if the
service subsequently recovers.

A failed output write cancels the owning watch, waits for its normal return and
exits with an error. It prints no later state or successful completion. The
consumer does not panic on a broken pipe or leave observation running until the
original deadline after detecting that error.

This can establish a real adapter observation and cooperative cancellation. An
unchanged observation window does not prove delivery of later state changes.
It does not establish audible playback, recording, Bluetooth connection/profile
support, routing controls or native desktop UI acceptance. Headsets and external
display audio require separate device-specific tests. Source and binary identity
must be recorded separately before treating a run as release evidence.
