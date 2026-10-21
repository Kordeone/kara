# Kara local testing packages

These artifacts are unsigned owner-testing builds, not production releases.
Application ID remains `io.github.kordeone.Kara`. Fully Quit existing native,
Flatpak and AppImage processes before switching builds: all use that same
single-instance bus name. Never delete application data merely to switch builds.

## Tools and isolation

Use Rust **1.99.1**, Cargo.lock, Python 3.9+, pkg-config, a C/C++ compiler,
xkbcommon headers, desktop-file-validate and appstreamcli. The pinned
`packaging/Containerfile` provides Ubuntu 22.04 / glibc 2.35 build tools in a
rootless container. Its package repositories are not snapshot-pinned; preserve
`builder-dpkg.txt` and the built image for this candidate. No bit-for-bit
reproducibility claim is made. No privileged container is required.

Build that image with Podman, mount only this feature source at `/source`, a
NEW build directory at `/work`, and the exact installed Rust toolchain at
`/opt/rust:ro`. Set CARGO_HOME=/work/cargo and copy only Cargo registry/git
caches into it, or fetch locked dependencies there. Do not mount production
XDG data. The initial apt/Cargo/runtime downloads require network; the actual
Rust builds below are offline. If using a localhost proxy, use a suitable
container proxy route; this run used host networking only for build downloads.
That does not add network permission to the resulting Flatpak.

## Shared payload

```sh
python3 packaging/build.py --destdir /work/AppDir --prefix /usr --offline
```

The helper verifies Rust's version, obtains the actual binary and generated
metadata paths from Cargo JSON, stages all official 32–512px icons, licenses,
font notices and a locked dependency inventory, then validates the installed
metadata. build.rs writes only under Cargo OUT_DIR. No `target/xdgen` convention
or working-directory-dependent installation is used. The 512px source icon is
byte-identical to accepted main; smaller copies are generated from it.

No new application notifications or reminders are implemented. Existing portal
access is retained; notification delivery is a separate packaged runtime check.

## Flatpak source build

Inside the builder, with private XDG directories (example root `/work/flatpak`):

```sh
export XDG_DATA_HOME=/work/flatpak/data
export XDG_CONFIG_HOME=/work/flatpak/config
export XDG_CACHE_HOME=/work/flatpak/cache
flatpak --user remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak --user install -y flathub org.freedesktop.Platform//25.08 org.freedesktop.Sdk//25.08
python3 packaging/flatpak/prepare.py /work/flatpak-prepared --toolchain /opt/rust
flatpak-builder --user --disable-rofiles-fuse --state-dir=/work/flatpak-state \
  --repo=/work/flatpak-repo /work/flatpak-build \
  /work/flatpak-prepared/io.github.kordeone.Kara.json
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
  /work/flatpak-repo /work/artifacts/Kara-0.1.0-testing-x86_64.flatpak \
  io.github.kordeone.Kara
```

The source-build and export commands require working Flatpak namespace sandboxes.
This Debian rootless container rejects the full sandbox with
`bwrap: Creating new namespace failed: Operation not permitted`. It also rejects
build-export's sandboxed icon validator. No `.flatpak` was produced here. Do not
use privileged mode, disable the sandbox or skip icon validation to bypass this.
Run the documented commands on a supported test host instead.

An optional standard prebuilt route accepts the already verified Ubuntu payload;
it still uses normal sandboxed export validation and is blocked here as well:

```sh
python3 packaging/build.py --destdir /work/flatpak-payload --prefix /app --offline
python3 packaging/flatpak/assemble.py --payload /work/flatpak-payload/app \
  --builddir /work/flatpak-export-build --repo /work/flatpak-export-repo \
  --output /work/artifacts/Kara-0.1.0-testing-x86_64.flatpak
```

Use new preparation/build paths, not an unrelated existing directory. Preparation
vendors exact registry and Git sources; the sandbox build uses frozen/offline
Cargo. The template supports SDK rust-stable, but that extension is rolling.
`--toolchain` explicitly supplies verified Rust 1.99.1 instead; no compiler is
installed in the final `/app` payload. Record SDK/runtime commits alongside the
candidate. New upstream runtime builds require revalidation, not blind reuse of
old results.

Permissions are Wayland, GPU rendering, the tray watcher and two exact COSMIC
settings watcher services. Four specific COSMIC toolkit/theme directories are
read-only to preserve system theme and updates. No broad home/host access,
network, X11, system bus, unrestricted session bus or writable shared COSMIC
configuration is granted. Portal-selected backup folders remain user-authorized;
Kara's own persisted data stays private in the Flatpak XDG data directory.

Do not replace the portal with broad filesystem permissions to make tests pass.
Selected writable directory grants, sync/atomic creation, retention identity,
permission revocation and full restart must pass on COSMIC. Locked libcosmic's
chooser has no parent-window identifier; verify stacking and cancellation.

### Owner installation, launch and uninstall

On the test machine with Flatpak available:

```sh
flatpak --user remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak --user install --bundle ./Kara-0.1.0-testing-x86_64.flatpak
flatpak info --user --show-permissions io.github.kordeone.Kara
flatpak run io.github.kordeone.Kara
flatpak --user uninstall io.github.kordeone.Kara
```

Do not add `--delete-data`; uninstall must preserve tasks and recovery archives.
The ordinary private data path is
`~/.var/app/io.github.kordeone.Kara/data/io.github.kordeone.Kara/tasks.json`.
Native data is not silently imported. Use Export/Import with disposable fixtures.
Prefer a separate test account for GUI acceptance; never seed production data.

## AppImage source build

Build the shared payload inside Ubuntu 22.04, not on Debian 13. Then:

```sh
python3 packaging/appimage/fetch-tools.py /work/tools
python3 packaging/appimage/build.py --appdir /work/AppDir --tools /work/tools \
  --epoch 1792542388 --output /work/artifacts/Kara-0.1.0-testing-x86_64.AppImage
```

Replace the epoch with the candidate source's recorded commit time when making
another build. appimagetool 1.9.1 and runtime 20251108 are checksum-pinned. A
changed download fails verification. Preserve tools, source/vendor archive and
runtime sources for reproducibility and licensing review. Do not silently
substitute a mutable `continuous` runtime.

Only Ubuntu libxkbcommon.so.0 is bundled, with notices. Host libgcc_s.so.1, libc,
the loader, GPU drivers and hardware-specific graphics libraries are not bundled.
This follows AppImage's host-library exclusion guidance and avoids overriding the
host graphics stack's compiler runtime.
Every payload ELF is checked against a maximum GLIBC_2.35 symbol policy. The
minimum supported testing baseline is x86_64 Ubuntu 22.04 / glibc 2.35, with
compatible host graphics libraries, Wayland/X11 session and session D-Bus.
This baseline is a build/link constraint, not proof that every such distribution
has passed GUI tests. Host Vulkan/EGL/Wayland/X11 loading still needs clean-host
acceptance. The AppImage runtime is static; FUSE mounting or extract-and-run
must work on the host. No root installation is required.

```sh
chmod +x ./Kara-0.1.0-testing-x86_64.AppImage
./Kara-0.1.0-testing-x86_64.AppImage
# If FUSE mounting is unavailable:
APPIMAGE_EXTRACT_AND_RUN=1 ./Kara-0.1.0-testing-x86_64.AppImage
```

Uninstall by removing only that downloaded AppImage, for example:

```sh
rm -- ./Kara-0.1.0-testing-x86_64.AppImage
```

Keep `~/.local/share/io.github.kordeone.Kara`, manual exports and recovery archives.
No automatic desktop integration or autostart is added.

## Validation and acceptance

Run the Rust checks in report.txt and `python3 packaging/test_packaging.py`.
Validate the staged metadata, actual package permissions, exported icon and
installed license tree. Headless extraction/loader/sandbox tests establish
structural readiness only. Required COSMIC checks are in report.txt; no Debian
GUI/portal/notification acceptance is claimed.

Dependency licensing evidence includes exact upstream texts when Cargo archives
omit them. drm-fourcc 2.2.0 has an MIT declaration but no full upstream text at
its locked revision; clarify that notice and complete runtime source/relink and
security review before any public distribution. These local artifacts do not
constitute a legal or dependency-security clearance.
