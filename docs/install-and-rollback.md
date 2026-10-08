# Install, audit, activate, recover

The three local VMs are independent clones of an installed Omarchy base. A flattened read-only copy of the base disk backs three private qcow2 overlays. Each VM has its own NVRAM, MAC address and loopback SSH port. Nothing in a clone writes the original disk. Runtime inventory, images, SSH keys, credentials and raw reports are excluded from Git.

Start only one lab VM at a time on a 16 GiB host. Each clone is configured for 3 GiB RAM and two vCPUs.

```sh
python3 vm/lab.py status
python3 vm/lab.py start lucid          # or noctalia / lucent
python3 vm/lab.py console lucid
python3 vm/lab.py shutdown lucid
```

The console opens Virtual Machine Manager on `qemu:///session`. The guest login is the same as the source VM; consult the local source-VM login note. No password or key is stored in this repository. Use Left Ctrl+Left Alt to release the VM keyboard grab.

`python3 vm/launch.py lucid` combines start and console. Shutdown uses the QEMU guest agent because a desktop can intercept the emulated ACPI power button. The installer prepares the agent inside QEMU/KVM guests. Each prepared overlay has a local qcow2 snapshot named `prepared`; inspect it only while the VM is shut off. Reverting it discards subsequent guest changes and should be a deliberate recovery action.

To reproduce the clones from another installed Omarchy VM:

```sh
python3 vm/lab.py create --base NAME --ssh-key /path/to/guest/key --user omarchy
python3 vm/lab.py start lucid
python3 vm/run.py --sync lucid
```

`create` requires a stopped base VM, exactly one qcow2 system disk and existing SSH access. It refuses to replace an existing inventory or base copy. Interrupted creation leaves its disk files for inspection; it never deletes them automatically. Its virtio NIC occupies PCI address 0x10 as in the audited base definition. Custom base hardware may need an XML adjustment.

If no installed Omarchy VM exists, follow the [official installation manual](https://omarchy.org/manual/). Attach the official ISO to a new empty VM, verify its checksum, and perform disk selection and installation interactively. The repository has no partitioning, formatting or disk-erasure automation. Complete a stock graphical login and establish SSH access before cloning.

Inside a test guest:

```sh
cd ~/desktop-lab
scripts/doctor.sh
scripts/install.sh lucid             # interactive by default
scripts/verify.sh --lock             # in a graphical terminal; unlock normally
python3 scripts/setup-look.py lucid  # VM reference-resolution preset
scripts/activate.sh lucid            # stages next-login integration
# Log out and back in.
scripts/verify.sh
```

Substitute `noctalia` for the second profile. The lock test must succeed before activation. `--yes` is allowed only on detected QEMU/KVM guests and performs no disk operations. The installer pins and checksums Lucid's source archive and validates the reviewed installer hash before executing it. Flags include `--no-hypr --no-apps --no-look --no-theming --no-wallpapers --no-plugins`; the lab manages its own shell-only wallpaper bridge and explicitly installs reviewed dependencies. Noctalia uses Arch's official `noctalia` package and validates its native v5 config with `noctalia config validate`.

Package operations use `pacman -S --needed`, never a database-only `-Sy`. Bring the guest to a consistent supported package set before installing. Exact installed package versions are recorded under `~/.local/state/desktop-lab/`; Arch repository contents themselves are not frozen by this repo.

For reproducible weather personalization, keep a private JSON file containing `name`, `latitude`, `longitude`, and optionally `timezone`. Stop `desktop-lab-shell.service`, run `python3 scripts/set-weather.py PROFILE /path/to/private-location.json`, then start the service again. This disables IP location detection and stores the selected city only in guest state. Subsequent installs reapply that private selection. Do not commit the JSON or screenshots revealing personal location. The prepared weather profiles also have a local `weather-ready` disk snapshot.

Backups live under `~/.local/state/desktop-lab/original`, with a manifest recording both existing files and paths that were absent. Re-running an installer never replaces the first backup. Original package inventories and startup checksums are recorded beside it. Installed applications are retained during rollback; review the before/after package inventories if you want to remove additions.

For normal rollback, run from a terminal or SSH:

```sh
cd ~/desktop-lab
scripts/rollback.sh
# Log out and back in, or reboot the guest.
```

This restores user configuration, startup modules, masks and unit files. It leaves backups and reports available. Avoid deleting the repository while a clone disk references `.local-vms/base.qcow2`.

For emergency manual recovery at a TTY, move `~/.config/default/hypr/autostart.lua` aside, remove the marked desktop-lab `dofile(...)` from `~/.config/hypr/autostart.lua`, restore the original `omarchy-sleep-lock.service` user override from the backup manifest, run `systemctl --user daemon-reload`, and log in again. Prefer `rollback.sh` because it knows whether these files existed originally. The independent VM base also allows discarding and recreating a damaged test overlay after explicitly reviewing its contents.

Lucent's prototype is launched manually from its VM application menu; the stock desktop is retained. Closing the prototype is its normal rollback.
