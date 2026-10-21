# AppImage continuous integration

## Scope and trust

`.github/workflows/appimage.yml` builds unsigned **testing artifacts**, never a
GitHub Release. It runs for pull requests targeting main, pushes to main, and
manual dispatch. Manual dispatch becomes available after the workflow exists on
the default branch; a reviewed PR is the first-run route before that integration.
No tag trigger, write permission, release token, secret, push, signing key or
publication step exists. Checkout does not persist credentials. Fork PRs run with
the ordinary read-only `pull_request` context, never `pull_request_target`.

The Ubuntu 22.04 hosted runner launches a builder from
`packaging/ci/Containerfile`. The container pins the Ubuntu image digest and the
signed Ubuntu archive snapshot `20261020T000000Z`, freezing apt's dependency
closure. A checksum-pinned Ubuntu CA package bootstraps HTTPS. Historical archive
expiry is ignored only for those snapshot sources; package signatures, hashes
and TLS checks remain enabled. Update the snapshot deliberately for security
maintenance, revalidate, and retain the package inventory. Snapshot availability
is an external service dependency, not an unlimited archival guarantee.

Rust 1.99.1's compiler, Cargo, standard library, rustfmt and Clippy are installed
from exact official dated archives with SHA-256 verification. The hashes were
resolved from the official manifest whose digest is recorded in tools.lock.json.
No rolling toolchain action or unauthenticated installer is executed. actionlint
1.7.12 is also pinned by hash; ShellCheck and metadata tools come from the frozen
apt snapshot. Official checkout/upload-artifact actions are pinned by full commit
SHA, with their release versions annotated in the workflow.

The host runner/kernel and Docker engine can receive updates. These pins provide
a repeatable build recipe, not a promise of independently byte-identical full
compilation or perpetual dependency availability. No shared Cargo/Actions cache
is restored. CI disables dev/test debug symbols and incremental compilation to
fit standard hosted runner storage; release settings remain unchanged. Initial
dependencies are fetched with Cargo.lock; all subsequent
Rust builds run offline. No binary from the developer's machine is used in CI.
The build runs as the host runner's unprivileged UID, not container root: existing
permission-denial tests must exercise real failures. Timezone data and Noto/DejaVu
fonts are installed from the same snapshot for DST and software layout tests.
For rootless Podman, also pass `--userns=keep-id` to preserve host UID ownership.

## Build and validation path

`build-appimage.sh NEW_BUILD_DIRECTORY` is the single CI entry point. It runs:

1. Locked source fetch; exact commit/time and tool/package inventory recording.
2. actionlint, ShellCheck, and the existing complete `packaging/validate.sh`:
   formatting, locked check, strict Clippy, tests, debug/release builds, isolated
   D-Bus regression, packaging tests, desktop entry and AppStream validation.
3. Focused CI tests, then the existing `packaging/build.py`,
   `packaging/appimage/fetch-tools.py` and `packaging/appimage/build.py`.
4. Extraction of the actual resulting AppImage without FUSE. Check ELF64 x86_64
   and type-2 headers, official icon bytes at all six sizes, launcher icon/link,
   desktop metadata, license installation, executable AppRun, exact permitted
   runtime library set, resolved ELF dependencies and numeric GLIBC <= 2.35.
   Check extracted payload symbol versions against assembly evidence.
5. SHA-256 generation and verification, then measured byte/MiB summary.

Every required check must pass before upload. No application GUI is launched.
Disposable XDG directories come from the existing validator. Native COSMIC,
portals, FUSE mounting and actual clean-host behavior need owner acceptance.

Local equivalent (Docker; Podman can substitute without privileged mode):

```sh
docker build -t kara-appimage-builder -f packaging/ci/Containerfile packaging/ci
ci_work=$(mktemp -d "${TMPDIR:-/var/tmp}/kara-ci.XXXXXXXX")
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/source:ro" -v "$ci_work:/work" \
  kara-appimage-builder bash /source/packaging/ci/build-appimage.sh /work/candidate
```

Run from the repository root. Use a fresh directory; do not mount real user data.
No hardcoded developer path, release target directory or external prebuilt Kara
binary is required. Logs and caches remain in the chosen build directory.

## Artifact conventions

Download container name:
`Kara-AppImage-x86_64-<full github.sha>-<run_attempt>`.
Files:

- `Kara-<Cargo version>-testing-<12-character source SHA>-x86_64.AppImage`
- Identical basename plus `.sha256`: standard `sha256sum --check` format, basename
  only; verify from the extracted download directory.
- `.AppImage.json`: builder checksums, actual size, pinned tools and glibc evidence.
- `.AppImage.validation.json`: actual extracted artifact verification.
- `build-environment.txt` and `summary.md`: source commit/time, tool/package versions,
  actual artifact size and checksum. The GitHub job summary repeats measurements.

The GitHub artifact is a ZIP containing these files, retained **7 days**. No extra
ZIP compression is requested for the compressed AppImage. ZIP download may lose
executable mode: run `chmod +x -- ./Kara-*.AppImage` only in a directory containing
this downloaded candidate. The artifact service digest covers the uploaded
archive; the `.sha256` covers the AppImage itself. A PR build normally identifies
the synthetic merge commit, not just the PR head. Read the recorded source SHA.
The earlier 13.56 MiB package is a reference only; each CI run measures its result.

## Owner first-run checklist

1. Review pins, workflow and packaging changes. Owner pushes/reviews a PR targeting
   main; no agent push is implied. Approve first-time fork workflows if required.
2. Confirm checkout SHA, Ubuntu snapshot, Rust version and read-only permissions.
   Verify every validation step succeeds and no release/tag appears.
3. Download the artifact before its 7-day expiry. Compare summary, actual size and
   `sha256sum --check *.sha256`. Keep metadata and package inventory with results.
4. On COSMIC, completely Quit previous Kara instances via tray before launching
   the downloaded candidate; all builds share the same single-instance identity.
   Prefer a disposable account/dataset. Verify icon, theme, module selector,
   Tasks/Calendar/Matrix, Matrix drag/drop, tray open/close/quit and second launch.
5. Check Export, Add/Replace import, recovery/clear safeguards and automatic backup
   folder persistence, including cancellation and inaccessible destinations.
6. Test both normal FUSE and `APPIMAGE_EXTRACT_AND_RUN=1` launch on x86_64 Ubuntu
   22.04/glibc 2.35 and CachyOS. Confirm graphics/runtime dependencies there.
7. Do not label a local container pass as a GitHub-hosted or graphical pass.

## Separate future release approval

Normal runs always produce testing artifacts. A future public release requires
explicit owner approval of the exact version, commit, tag and graphical/license/
security results. Only after that decision should a separate release workflow be
reviewed, with a protected approval environment and minimal job-specific release
write permission. Do not attach public-release behavior to this workflow. No
release workflow or tag trigger is enabled in this milestone. Preserve source,
notices, dependency inventory and checksums with any approved release candidate.

## Flatpak CI readiness

The existing Freedesktop 25.08 manifest is ready for an initial **source-build
trial**; a complete sandboxed build/export has not yet passed on this Debian
rootless host (`bwrap: Creating new namespace failed: Operation not permitted`).
Do not nest that failing container or alter the Debian host's sandbox settings.
No Flatpak workflow is enabled until a legitimate runner sandbox is verified.

The smallest follow-up is a separate read-only workflow on a disposable native
Ubuntu 22.04 GitHub-hosted VM, outside Docker:

1. Pin checkout and toolchain as above. Install flatpak, flatpak-builder and their
   build dependencies on that ephemeral runner. Check a normal unprivileged
   bubblewrap user/mount namespace first; stop if unavailable, never disable it.
2. Use temporary XDG data/config/cache/runtime paths. Add the signed Flathub remote
   only to that temporary user installation, install Freedesktop Platform/SDK 25.08
   and record their exact commits (pin those commits for reproducibility).
3. Fetch locked Cargo sources. Run `packaging/flatpak/prepare.py` with a fresh
   runner-temp output outside the checkout and `--toolchain /opt/rust` pointing
   to the same checksum-verified 1.99.1 toolchain. This removes the manifest's
   rolling Rust extension and vendors registry/Git sources for offline building.
4. Run the existing documented `flatpak-builder --user --disable-rofiles-fuse`
   source build with a fresh build/state/repository directory. That flag avoids a
   FUSE optimization; it does not disable bubblewrap or icon validation.
5. Normal `flatpak build-bundle`, permission/metadata inspection, SHA-256 and
   upload-artifact with seven-day retention. No Flathub publishing or release.

Commands and the exact permission allowlist remain in `packaging/README.md` and
`packaging/test_packaging.py`. Host-level Flatpak tool installation here is only
proposed for an ephemeral GitHub runner; none is authorized on the Debian host.
A hosted build/export, portal directory grants across restarts, tray filtering,
notifications and COSMIC theme behavior remain unverified. Public Flathub work
also needs accepted screenshots/release metadata, dependency/security review,
drm-fourcc's missing upstream full MIT notice clarification, and runtime source/
relink obligations. Do not broaden filesystem or session-bus permissions to make
acceptance pass.

## References

- [GitHub secure workflow guidance](https://docs.github.com/en/actions/reference/security/secure-use)
- [GitHub event behavior](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows)
- [Official checkout action](https://github.com/actions/checkout)
- [Official upload-artifact action](https://github.com/actions/upload-artifact)
- [Ubuntu snapshot service](https://snapshot.ubuntu.com/)
- [Official Rust 1.99.1 manifest](https://static.rust-lang.org/dist/channel-rust-1.99.1.toml)
- [Flatpak's Actions guidance](https://github.com/flatpak/flatpak-github-actions)

- [GitHub hosted runner resource limits](https://docs.github.com/en/enterprise-cloud%40latest/actions/reference/runners/github-hosted-runners)
