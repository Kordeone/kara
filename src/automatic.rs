// SPDX-License-Identifier: MPL-2.0
//! Running-process scheduling and an explicit catalogue of owned local archives.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

pub const RETENTION: u16 = 10;
fn default_retention() -> u16 {
    RETENTION
}
fn is_default_retention(value: &u16) -> bool {
    *value == RETENTION
}
pub fn retention_count(input: &str) -> Result<u16, String> {
    if input.is_empty() || !input.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Keep last must be a whole number from 1 to 100.".into());
    }
    let value = input
        .parse::<u16>()
        .map_err(|_| "Keep last must be a whole number from 1 to 100.")?;
    validate_retention(value)?;
    Ok(value)
}
fn validate_retention(value: u16) -> Result<(), String> {
    if !(1..=100).contains(&value) {
        return Err("Keep last must be from 1 to 100.".into());
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub enabled: bool,
    pub interval: String,
    pub last_success: Option<i64>,
    // Omitting the default preserves checksums of existing version-2 backups.
    #[serde(
        default = "default_retention",
        skip_serializing_if = "is_default_retention"
    )]
    pub retention: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<PathBuf>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "PT6H".into(),
            last_success: None,
            retention: RETENTION,
            destination: None,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        schedule_seconds(&self.interval, 0)?;
        if self.destination.as_ref().is_some_and(|p| !p.is_absolute()) {
            return Err("Backup destination must be an absolute folder path.".into());
        }
        validate_retention(self.retention)?;
        if self
            .last_success
            .is_some_and(|t| t < 0 || jiff::Timestamp::from_second(t).is_err())
        {
            return Err("Invalid last backup timestamp.".into());
        }
        Ok(())
    }
}
pub fn interval_seconds(input: &str) -> Result<u64, String> {
    let error = "Use a positive ISO 8601 duration in whole seconds: PT30M, PT6H, P1D or P1W. Months and years are not fixed intervals.";
    if !input.starts_with('P') || input.trim() != input {
        return Err(error.into());
    }
    let span: jiff::Span = input.parse().map_err(|_| error)?;
    if span.get_years() != 0 || span.get_months() != 0 || span.signum() <= 0 {
        return Err(error.into());
    }
    let duration = span
        .to_duration(jiff::SpanRelativeTo::days_are_24_hours())
        .map_err(|_| error)?;
    if duration.subsec_nanos() != 0 || duration.as_secs() <= 0 {
        return Err(error.into());
    }
    Ok(duration.as_secs() as u64)
}
/// New configurations have a one-minute floor; legacy intervals remain readable.
pub const MIN_SECONDS: u64 = 60;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unit {
    Seconds,
    Minutes,
    #[default]
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}
impl Unit {
    pub const ALL: [Self; 7] = [
        Self::Seconds,
        Self::Minutes,
        Self::Hours,
        Self::Days,
        Self::Weeks,
        Self::Months,
        Self::Years,
    ];
    pub const LABELS: [&str; 7] = [
        "Seconds (s)",
        "Minutes (m)",
        "Hours (h)",
        "Days (d)",
        "Weeks (w)",
        "Months (M)",
        "Years (y)",
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|u| *u == self).unwrap()
    }
    fn iso(self, amount: u64) -> String {
        match self {
            Self::Seconds => format!("PT{amount}S"),
            Self::Minutes => format!("PT{amount}M"),
            Self::Hours => format!("PT{amount}H"),
            Self::Days => format!("P{amount}D"),
            Self::Weeks => format!("P{amount}W"),
            Self::Months => format!("P{amount}M"),
            Self::Years => format!("P{amount}Y"),
        }
    }
}
pub fn configuration_interval(amount: &str, unit: Unit) -> Result<String, String> {
    if amount.is_empty() || !amount.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Enter a positive whole number.".into());
    }
    let number: u64 = amount.parse().map_err(|_| "Interval is too large.")?;
    if number == 0
        || number > 1_000_000
        || (unit == Unit::Years && number > 100)
        || (unit == Unit::Months && number > 1200)
    {
        return Err("Enter 1–1,000,000 (up to 100 years or 1,200 months).".into());
    }
    let iso = unit.iso(number);
    if (if calendar_span(&iso).is_some() {
        schedule_seconds(&iso, jiff::Timestamp::now().as_second())?
    } else {
        interval_seconds(&iso)?
    }) < MIN_SECONDS
    {
        return Err("Use an interval of at least one minute (60 seconds).".into());
    }
    Ok(iso)
}
fn calendar_span(input: &str) -> Option<jiff::Span> {
    let number = input.strip_prefix('P')?;
    let (digits, suffix) = number.split_at(number.len().checked_sub(1)?);
    if !matches!(suffix, "M" | "Y")
        || digits.is_empty()
        || !digits.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let amount: i32 = digits.parse().ok()?;
    if amount <= 0 {
        return None;
    }
    // Parsing delegates range checks to the same calendar library as arithmetic.
    input.parse().ok()
}
pub fn interval_controls(input: &str) -> Result<(String, Unit), String> {
    if calendar_span(input).is_some() {
        let unit = if input.ends_with('Y') {
            Unit::Years
        } else {
            Unit::Months
        };
        return Ok((input[1..input.len() - 1].into(), unit));
    }
    let seconds = interval_seconds(input)?;
    // Preserve a saved single-unit choice (60 seconds must reopen as seconds).
    // Legacy composite durations fall back to an equivalent whole-number unit.
    for (unit, prefix, suffix) in [
        (Unit::Seconds, "PT", 'S'),
        (Unit::Minutes, "PT", 'M'),
        (Unit::Hours, "PT", 'H'),
        (Unit::Days, "P", 'D'),
        (Unit::Weeks, "P", 'W'),
    ] {
        if let Some(amount) = input
            .strip_prefix(prefix)
            .and_then(|s| s.strip_suffix(suffix))
            && !amount.is_empty()
            && amount.bytes().all(|c| c.is_ascii_digit())
        {
            return Ok((amount.into(), unit));
        }
    }
    for (unit, divisor) in [
        (Unit::Weeks, 604800),
        (Unit::Days, 86400),
        (Unit::Hours, 3600),
        (Unit::Minutes, 60),
        (Unit::Seconds, 1),
    ] {
        if seconds % divisor == 0 {
            return Ok(((seconds / divisor).to_string(), unit));
        }
    }
    unreachable!()
}
fn schedule_seconds_in(input: &str, wall: i64, zone: jiff::tz::TimeZone) -> Result<u64, String> {
    if let Some(span) = calendar_span(input) {
        let start = jiff::Timestamp::from_second(wall)
            .map_err(|e| e.to_string())?
            .to_zoned(zone);
        let end = start
            .checked_add(span)
            .map_err(|e| format!("Calendar interval is out of range: {e}"))?;
        return u64::try_from(end.timestamp().as_second().saturating_sub(wall))
            .map_err(|_| "Invalid calendar interval.".into());
    }
    interval_seconds(input).map(|s| s.max(MIN_SECONDS))
}
pub fn schedule_seconds(input: &str, wall: i64) -> Result<u64, String> {
    schedule_seconds_in(input, wall, jiff::tz::TimeZone::system())
}
/// A disposable probe verifies file creation, read/write, sync and cleanup without
/// touching existing files or requesting privilege. Rechecked at Apply and backup.
pub fn verify_destination(path: &Path) -> Result<(), String> {
    let check = || -> Result<(), String> {
        if !path.is_absolute()
            || !fs::symlink_metadata(path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_dir()
        {
            return Err("Choose an existing real folder, not a file or symbolic link.".into());
        }
        let mut probe = tempfile::Builder::new()
            .prefix(".kara-access-")
            .tempfile_in(path)
            .map_err(|e| e.to_string())?;
        probe
            .write_all(b"Kara backup access check")
            .and_then(|()| probe.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        if fs::read(probe.path()).map_err(|e| e.to_string())? != b"Kara backup access check" {
            return Err("Folder verification failed.".into());
        }
        probe.close().map_err(|e| e.to_string())?;
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        // Refuse a foreign/corrupt index instead of replacing it later.
        catalogue(path)?;
        Ok(())
    };
    check().map_err(|e| format!("Cannot use backup folder {}: {e}. Choose a writable folder; no elevated privileges are requested.", path.display()))
}
/// CLOCK_BOOTTIME includes suspend and does not jump with wall-clock corrections.
pub fn boot_seconds() -> Result<u64, String> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: time points to a fully initialized, writable timespec, and the
    // Linux clock ID is fixed. The return code is checked before using it.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0 {
        return Err(format!(
            "Cannot read backup clock: {}",
            std::io::Error::last_os_error()
        ));
    }
    u64::try_from(time.tv_sec).map_err(|_| "Invalid backup clock.".into())
}
#[derive(Default)]
pub struct Scheduler {
    next_boot: Option<u64>,
    retry_boot: Option<u64>,
}
impl Scheduler {
    pub fn due(&mut self, prefs: &Preferences, wall: i64, boot: u64) -> Result<bool, String> {
        if !prefs.enabled {
            *self = Self::default();
            return Ok(false);
        }
        let interval = schedule_seconds(&prefs.interval, prefs.last_success.unwrap_or(wall))?;
        if self.retry_boot.is_some_and(|retry| boot < retry) {
            return Ok(false);
        }
        let elapsed = prefs.last_success.map_or(interval, |last| {
            // A timestamp in the future on restart must not stall indefinitely.
            if wall < last {
                interval
            } else {
                wall.saturating_sub(last) as u64
            }
        });
        let deadline = *self
            .next_boot
            .get_or_insert_with(|| boot.saturating_add(interval.saturating_sub(elapsed)));
        // Once initialized, elapsed time is authoritative. Wall-clock jumps
        // cannot create extra backups or postpone a deadline. Only restart needs
        // the persisted wall timestamp to establish its first remaining interval.
        Ok(boot >= deadline)
    }
    pub fn finished(&mut self, interval: u64, boot: u64, success: bool) {
        if success {
            self.next_boot = Some(boot.saturating_add(interval));
            self.retry_boot = None;
        } else {
            self.retry_boot = Some(boot.saturating_add(60));
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    version: u32,
    files: Vec<OwnedArchive>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedArchive {
    name: String,
    device: u64,
    inode: u64,
    changed_seconds: i64,
    changed_nanos: i64,
}
impl OwnedArchive {
    fn matches(&self, metadata: &fs::Metadata) -> bool {
        metadata.file_type().is_file()
            && self.device == metadata.dev()
            && self.inode == metadata.ino()
            && self.changed_seconds == metadata.ctime()
            && self.changed_nanos == metadata.ctime_nsec()
    }
}
fn valid_name(name: &str) -> bool {
    name.strip_prefix("auto-")
        .and_then(|s| s.strip_suffix(".kara-backup"))
        .is_some_and(|s| {
            s.len() == 32
                && s.bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        })
}
fn catalogue(dir: &Path) -> Result<(Catalogue, Option<Vec<u8>>), String> {
    let path = dir.join("index.json");
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err("Automatic backup catalogue is not a regular file.".into());
    }
    let bytes = match File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(65537)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 65536 {
                return Err("Automatic backup catalogue is too large.".into());
            }
            Some(bytes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let data: Catalogue = bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|e| format!("Invalid automatic backup catalogue: {e}"))?
        .unwrap_or(Catalogue {
            version: 1,
            files: Vec::new(),
        });
    let mut names = std::collections::HashSet::new();
    if data.version != 1
        || data
            .files
            .iter()
            .any(|entry| !valid_name(&entry.name) || !names.insert(&entry.name))
    {
        return Err(
            "Unsupported or damaged automatic backup catalogue; archives preserved.".into(),
        );
    }
    Ok((data, bytes))
}
fn save_catalogue(dir: &Path, data: &Catalogue, expected: Option<&[u8]>) -> Result<(), String> {
    let path = dir.join("index.json");
    let current = fs::read(&path)
        .map(Some)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(e)
            }
        })
        .map_err(|e| e.to_string())?;
    if current.as_deref() != expected {
        return Err("Automatic backup catalogue changed externally.".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    temp.write_all(&serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
pub fn publish(dir: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    if fs::symlink_metadata(dir).is_ok_and(|m| !m.file_type().is_dir()) {
        return Err("Automatic backup directory is not a real directory.".into());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| e.to_string())?;
    let (mut index, expected) = catalogue(dir)?;
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let name = format!(
        "auto-{}.kara-backup",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let path = dir.join(&name);
    crate::backup::export(&path, bytes)?;
    if fs::read(&path).map_err(|e| e.to_string())? != bytes {
        return Err("Automatic backup verification failed.".into());
    }
    crate::backup::read(&path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    index.files.push(OwnedArchive {
        name,
        device: metadata.dev(),
        inode: metadata.ino(),
        changed_seconds: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
    });
    save_catalogue(dir, &index, expected.as_deref())?;
    Ok(path)
}
pub fn retain(dir: &Path, keep: u16) -> Result<(), String> {
    validate_retention(keep)?;
    if !fs::symlink_metadata(dir)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_dir()
    {
        return Err("Automatic backup directory is not a real directory; files preserved.".into());
    }
    let (mut index, expected) = catalogue(dir)?;
    let excess = index.files.len().saturating_sub(usize::from(keep));
    // Only catalogue-owned, regular, valid backups are candidates. Manual files,
    // recovery files, unknown files, symlinks and damaged archives are preserved.
    for archive in &index.files[..excess] {
        let path = dir.join(&archive.name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if archive.matches(&meta) => {
                crate::backup::read(&path)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            _ => {
                return Err(
                    "Retention found a replaced or nonregular archive; files preserved.".into(),
                );
            }
        }
    }
    for archive in index.files.drain(..excess) {
        let path = dir.join(&archive.name);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| !archive.matches(&metadata)) {
            return Err("Archive changed during retention; file preserved.".into());
        }
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    save_catalogue(dir, &index, expected.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_units_minimum_and_legacy_controls_are_unambiguous() {
        for (amount, unit, expected) in [
            ("60", Unit::Seconds, "PT60S"),
            ("1", Unit::Minutes, "PT1M"),
            ("6", Unit::Hours, "PT6H"),
            ("1", Unit::Days, "P1D"),
            ("1", Unit::Weeks, "P1W"),
            ("1", Unit::Months, "P1M"),
            ("1", Unit::Years, "P1Y"),
        ] {
            assert_eq!(configuration_interval(amount, unit).unwrap(), expected);
            let (value, parsed_unit) = interval_controls(expected).unwrap();
            assert_eq!(
                schedule_seconds(&configuration_interval(&value, parsed_unit).unwrap(), 0).unwrap(),
                schedule_seconds(expected, 0).unwrap()
            );
        }
        for amount in [
            "",
            "0",
            "-1",
            "+1",
            "1.5",
            " 2",
            "2 ",
            "1000001",
            "18446744073709551616",
        ] {
            assert!(
                configuration_interval(amount, Unit::Hours).is_err(),
                "{amount}"
            );
        }
        assert!(configuration_interval("59", Unit::Seconds).is_err());
        assert!(configuration_interval("101", Unit::Years).is_err());
        assert!(configuration_interval("1201", Unit::Months).is_err());
        assert_eq!(
            interval_controls("P1DT2H3M4S").unwrap(),
            ("93784".into(), Unit::Seconds)
        );
        assert_eq!(
            interval_controls("PT0.5H").unwrap(),
            ("30".into(), Unit::Minutes)
        );
        assert_eq!(
            interval_controls("PT1S").unwrap(),
            ("1".into(), Unit::Seconds)
        );
        assert_eq!(
            interval_controls("PT60S").unwrap(),
            ("60".into(), Unit::Seconds)
        );
        assert_eq!(
            interval_controls("PT120M").unwrap(),
            ("120".into(), Unit::Minutes)
        );
        assert_eq!(schedule_seconds("PT1S", 0).unwrap(), 60); // legacy readable, safe runtime minimum
        for invalid in ["P0M", "P-1M", "P1M1D", "P1Y1M", "P0Y"] {
            assert!(schedule_seconds(invalid, 0).is_err());
        }
    }
    #[test]
    fn calendar_intervals_constrain_month_ends_leap_years_and_observe_local_dst() {
        for (start, interval, end) in [
            (
                "2024-01-31T09:00:00-05:00[America/New_York]",
                "P1M",
                "2024-02-29T09:00:00-05:00[America/New_York]",
            ),
            (
                "2023-01-31T09:00:00-05:00[America/New_York]",
                "P1M",
                "2023-02-28T09:00:00-05:00[America/New_York]",
            ),
            (
                "2024-02-29T09:00:00-05:00[America/New_York]",
                "P1Y",
                "2025-02-28T09:00:00-05:00[America/New_York]",
            ),
            (
                "2024-02-10T09:00:00-05:00[America/New_York]",
                "P1M",
                "2024-03-10T09:00:00-04:00[America/New_York]",
            ),
            (
                "2024-12-31T09:00:00+00:00[UTC]",
                "P2M",
                "2025-02-28T09:00:00+00:00[UTC]",
            ),
        ] {
            let start: jiff::Zoned = start.parse().unwrap();
            let end: jiff::Zoned = end.parse().unwrap();
            assert_eq!(
                schedule_seconds_in(
                    interval,
                    start.timestamp().as_second(),
                    start.time_zone().clone()
                )
                .unwrap(),
                (end.timestamp().as_second() - start.timestamp().as_second()) as u64
            );
        }
    }
    #[test]
    fn calendar_scheduler_restart_and_clock_changes_do_not_replay_missed_periods() {
        let mut p = prefs(None);
        p.interval = "P1M".into();
        let wall = "2024-01-31T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_second();
        let duration = schedule_seconds(&p.interval, wall).unwrap();
        let mut running = Scheduler::default();
        assert!(running.due(&p, wall, 100).unwrap());
        running.finished(duration, 100, true);
        p.last_success = Some(wall);
        assert!(!running.due(&p, wall + 1_000_000_000, 101).unwrap());
        assert!(!running.due(&p, wall - 10000, 102).unwrap());
        assert!(running.due(&p, wall, 100 + duration).unwrap());
        assert!(
            !Scheduler::default()
                .due(&p, wall + duration as i64 - 1, 1)
                .unwrap()
        );
        let mut restarted = Scheduler::default();
        let late = wall + duration as i64 * 5;
        assert!(restarted.due(&p, late, 10).unwrap());
        p.last_success = Some(late);
        restarted.finished(schedule_seconds(&p.interval, late).unwrap(), 10, true);
        assert!(!restarted.due(&p, late, 11).unwrap());
    }
    #[test]
    fn destination_probe_is_disposable_and_rejects_denied_missing_and_foreign_paths() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().unwrap();
        assert!(verify_destination(temp.path()).is_ok());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        let file = temp.path().join("file");
        fs::write(&file, b"keep").unwrap();
        assert!(verify_destination(&file).is_err());
        assert!(verify_destination(&temp.path().join("missing")).is_err());
        assert!(verify_destination(Path::new("relative")).is_err());
        let link = temp.path().join("link");
        symlink(temp.path(), &link).unwrap();
        assert!(verify_destination(&link).is_err());
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let denied = verify_destination(temp.path());
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(denied.unwrap_err().contains("writable folder"));
        let foreign = temp.path().join("index.json");
        fs::write(&foreign, b"foreign file").unwrap();
        assert!(verify_destination(temp.path()).is_err());
        assert_eq!(fs::read(foreign).unwrap(), b"foreign file");
        assert_eq!(fs::read(file).unwrap(), b"keep");
    }
    #[test]
    fn retention_count_is_bounded_and_custom_cleanup_preserves_unowned_files() {
        for value in ["1", "10", "100"] {
            assert!(retention_count(value).is_ok());
        }
        for value in ["", "0", "101", "-1", "+1", "1.5", " 2", "2 ", "65536"] {
            assert!(retention_count(value).is_err(), "{value}");
        }
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let bytes = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
        for _ in 0..6 {
            publish(dir, bytes).unwrap();
        }
        let manual = dir.join("manual.kara-backup");
        crate::backup::export(&manual, bytes).unwrap();
        let recovery = dir.join("kara-before-clear-protected.kara-backup");
        crate::backup::export(&recovery, bytes).unwrap();
        assert!(retain(dir, 0).is_err());
        assert_eq!(catalogue(dir).unwrap().0.files.len(), 6);
        retain(dir, 2).unwrap();
        assert_eq!(catalogue(dir).unwrap().0.files.len(), 2);
        assert_eq!(fs::read(&manual).unwrap(), bytes);
        assert_eq!(fs::read(&recovery).unwrap(), bytes);
    }
    #[test]
    fn iso_intervals_accept_fixed_units_and_reject_invalid_or_ambiguous_values() {
        for (text, seconds) in [
            ("PT30M", 1800),
            ("PT6H", 21600),
            ("P1D", 86400),
            ("P1W", 604800),
            ("P1DT2H3M4S", 93784),
            ("PT1S", 1),
            ("PT0.5H", 1800),
        ] {
            assert_eq!(interval_seconds(text).unwrap(), seconds, "{text}");
        }
        for text in [
            "",
            "6h",
            "30 minutes",
            " PT1H",
            "PT1H ",
            "-PT1H",
            "PT0S",
            "P0D",
            "P1M",
            "P1Y",
            "PT0.5S",
            "PT",
            "P",
            "PT-5M",
            "PT1Q",
            "P9999999999999999999999W",
        ] {
            assert!(interval_seconds(text).is_err(), "{text}");
        }
        assert!(boot_seconds().unwrap() > 0);
    }
    fn prefs(last: Option<i64>) -> Preferences {
        Preferences {
            enabled: true,
            interval: "PT30M".into(),
            last_success: last,
            retention: RETENTION,
            destination: None,
        }
    }
    #[test]
    fn scheduler_coalesces_overdue_restart_and_suspend_without_replaying_misses() {
        let mut scheduler = Scheduler::default();
        let mut prefs = prefs(Some(1000));
        assert!(!scheduler.due(&prefs, 1100, 10).unwrap());
        assert!(scheduler.due(&prefs, 10000, 8910).unwrap());
        prefs.last_success = Some(10000);
        scheduler.finished(1800, 8910, true);
        for boot in 8910..8920 {
            assert!(!scheduler.due(&prefs, 10000, boot).unwrap());
        }
        let mut restarted = Scheduler::default();
        assert!(!restarted.due(&prefs, 10100, 50).unwrap());
        assert!(restarted.due(&prefs, 11800, 1750).unwrap());
        let mut overdue = Scheduler::default();
        assert!(overdue.due(&prefs, 100000, 1).unwrap());
        prefs.last_success = Some(100000);
        overdue.finished(1800, 1, true);
        assert!(!overdue.due(&prefs, 100000, 2).unwrap());
    }
    #[test]
    fn backward_forward_clocks_disabled_settings_and_failed_attempts_are_safe() {
        let mut scheduler = Scheduler::default();
        let mut p = prefs(Some(1000));
        assert!(!scheduler.due(&p, 1100, 100).unwrap());
        assert!(!scheduler.due(&p, 100000, 101).unwrap()); // forward clock jump, no elapsed interval
        assert!(!scheduler.due(&p, 500, 200).unwrap());
        assert!(scheduler.due(&p, 500, 1800).unwrap()); // boottime deadline despite backward wall clock
        scheduler.finished(1800, 1800, false);
        assert!(!scheduler.due(&p, 100000, 1859).unwrap());
        assert!(scheduler.due(&p, 100000, 1860).unwrap());
        p.last_success = Some(100000);
        scheduler.finished(1800, 1860, true);
        assert!(!scheduler.due(&p, 100000, 1861).unwrap());
        assert!(Scheduler::default().due(&p, 500, 1).unwrap()); // future timestamp after restart: once
        p.enabled = false;
        assert!(!scheduler.due(&p, 1000000, 99999).unwrap());
        assert!(Scheduler::default().due(&prefs(None), 0, 1).unwrap());
    }
    #[test]
    fn catalogue_retention_removes_only_owned_valid_archives() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("automatic-backups");
        let bytes = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
        let mut owned = Vec::new();
        for _ in 0..12 {
            owned.push(publish(&dir, bytes).unwrap());
            retain(&dir, RETENTION).unwrap();
        }
        assert!(!owned[0].exists() && !owned[1].exists());
        assert!(owned[2..].iter().all(|p| p.exists()));
        let manual = dir.join("manual.kara-backup");
        crate::backup::export(&manual, bytes).unwrap();
        let looks_owned = dir.join(format!("auto-{}.kara-backup", "f".repeat(32)));
        crate::backup::export(&looks_owned, bytes).unwrap();
        let recovery = dir.join("kara-before-clear-protected.kara-backup");
        crate::backup::export(&recovery, bytes).unwrap();
        publish(&dir, bytes).unwrap();
        retain(&dir, RETENTION).unwrap();
        assert!(manual.exists() && looks_owned.exists() && recovery.exists());
        assert_eq!(
            catalogue(&dir).unwrap().0.files.len(),
            usize::from(RETENTION)
        );
        assert!(
            fs::read_dir(&dir).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp"))
        );
    }
    #[test]
    fn corrupt_catalogues_bad_writes_and_symlinks_never_destroy_archives() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("automatic-backups");
        let bytes = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
        let archive = publish(&dir, bytes).unwrap();
        let count = fs::read_dir(&dir).unwrap().count();
        assert!(publish(&dir, b"{}").is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), count);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        let denied = publish(&dir, bytes);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(denied.is_err());
        assert_eq!(fs::read(&archive).unwrap(), bytes);
        fs::write(dir.join("index.json"), b"damaged").unwrap();
        assert!(publish(&dir, bytes).is_err());
        assert!(retain(&dir, RETENTION).is_err());
        assert_eq!(fs::read(&archive).unwrap(), bytes);
        let link = temp.path().join("linked");
        symlink(&dir, &link).unwrap();
        assert!(publish(&link, bytes).is_err());
    }
    #[test]
    fn manual_export_reusing_an_old_owned_filename_is_never_deleted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let bytes = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
        let oldest = publish(dir, bytes).unwrap();
        for _ in 0..10 {
            publish(dir, bytes).unwrap();
        }
        fs::remove_file(&oldest).unwrap();
        crate::backup::export(&oldest, bytes).unwrap(); // same content, different file identity
        assert!(retain(dir, RETENTION).is_err());
        assert_eq!(fs::read(&oldest).unwrap(), bytes);
        assert_eq!(catalogue(dir).unwrap().0.files.len(), 11);
    }
    #[test]
    fn corrupt_owned_archive_blocks_retention_without_touching_manual_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let bytes = include_bytes!("../tests/fixtures/backup-v1.kara-backup");
        let oldest = publish(dir, bytes).unwrap();
        for _ in 0..10 {
            publish(dir, bytes).unwrap();
        }
        fs::write(&oldest, b"damaged archive").unwrap();
        assert!(retain(dir, RETENTION).is_err());
        assert_eq!(fs::read(&oldest).unwrap(), b"damaged archive");
        assert_eq!(catalogue(dir).unwrap().0.files.len(), 11);
    }
}
