# System readings

The system widget starts with placeholders until a complete memory sample arrives.
CPU usage requires two complete samples: the first shows “Collecting…” rather than
an invented idle reading. Counter resets, decreasing counters and zero-length
intervals establish a new baseline. A measured idle interval still shows 0%.

`SystemPort` returns a validated `SystemSnapshot`; an optional CPU percentage is
separate from unavailable memory. `SystemProbe` reads bounded Linux aggregate CPU
and memory data, rejects incomplete or inconsistent fields, and resets its CPU
baseline after read or parse failure. Guest CPU counters are already included in
user/nice time and are not counted twice.

The desktop owns observation status separately from the last good sample. A failed
observation shows “Unavailable” before any sample, or muted “Last reading” values
after one. Recovery redraws even when its values equal the previous sample.
Repeated errors update diagnostics without redrawing identical presentation.
The existing five-second worker and per-output widget-group redraw routing remain
in place. Views perform no sampling or file access.

Owned file fixtures cover parser boundaries and recovery; domain and desktop
tests cover invalid samples, availability transitions, hidden widgets and output
routing. Six visual fixtures cover loading, unavailable, warming, ready, stale
and recovered states. Their current evidence is recorded in the
[system observation report](../reports/measurements/lucent-system-observation-polish.json).
Native interaction and resource acceptance remain pending while the VM is off.
