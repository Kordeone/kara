#!/bin/sh
# Headless regressions only. Never use production XDG data for package testing.
set -eu
root=$(mktemp -d "${TMPDIR:-/var/tmp}/kara-package-validation.XXXXXXXX")
trap 'rm -rf -- "$root"' EXIT HUP INT TERM
export TMPDIR="$root"
export XDG_DATA_HOME="$root/data" XDG_CONFIG_HOME="$root/config"
export XDG_CACHE_HOME="$root/cache" XDG_STATE_HOME="$root/state"
export XDG_RUNTIME_DIR="$root/runtime"
mkdir -m 700 "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
cargo build --release --locked
python3 packaging/ci/check-identity.py
KARA_PRIVATE_TRAY_TEST=1 dbus-run-session -- cargo test --locked \
  tray::tests::dbus_protocol_registration_recovery_and_single_instance_transport \
  -- --ignored --exact --nocapture
python3 packaging/test_packaging.py
appstreamcli validate --no-net resources/app.metainfo.xml
desktop-file-validate resources/app.desktop
