# Desktop settings persistence

`SettingsPort` keeps loading and saving separate from the desktop UI. The
client's `persistence::Persistence` owns the ordered settings queue: one accepted
write finishes while newer edits replace its one pending snapshot. Adapter
completion acknowledges the saved revision. A failed save keeps the edited
aggregate in memory and waits for an explicit retry unless a newer save was
already requested. This does not promise a shutdown flush or power-loss durability.

`JsonSettings` stores `desktop.json` in the Lucent XDG state directory. Its public
path remains configurable, and the public adapter is still exported as
`lucent_services::JsonSettings`. Existing parent directory permissions and
intermediate configured directory links are preserved.

Loading requires a regular file of at most 4 MiB. The adapter opens the final
pathname without following a symbolic link and uses nonblocking admission so a
FIFO cannot stall startup waiting for a writer. It checks the opened file and
reads at most the limit plus one byte to reject a file that grows during reading.
A missing file supplies defaults. A malformed, unsupported, oversized, linked or
nonregular file returns an error without modifying the original. The desktop
keeps saving disabled when loading fails.

Loading and saving both apply the pure settings validation use case. Notes have
one shared domain limit, `NOTES_MAX_BYTES`: 8,192 UTF-8 bytes. The ordinary notes
editor uses that same limit for edit admission. Invalid saved notes are rejected
before they reach renderer preparation; validation never truncates their file.

Saving validates the aggregate and bounds its serialized JSON before touching
storage. Ordinary documents retain readable indentation. If that whitespace
would exceed the 4 MiB limit, serialization clears and reuses the same bounded
buffer for compact JSON. A compact document within the limit can therefore be
loaded and saved again; a compact encoding beyond the limit still fails without
changing storage. It creates a unique exclusive temporary file beside the destination
with mode `0600`, writes the complete candidate, then renames it into place.
New parent directories use mode `0700`; existing directories are not changed.
Existing symbolic-link or nonregular destinations are rejected. A stale legacy
`desktop.tmp` is neither followed nor removed. Failed attempts clean up their
own candidate, leaving the destination intact. Readers therefore see a complete
old or new document, while the client remains responsible for ordering writes.

Owned adapter tests cover the byte limits, compact fallback round trips,
UTF-8 notes admission, preserved
invalid files, symbolic links, a real FIFO, private replacement permissions,
stale temporary links and candidate cleanup. Native desktop restart and
filesystem-failure acceptance remain separate checks.
