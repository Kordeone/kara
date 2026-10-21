# Kara worktree consolidation record

## Scope and preserved Git store

Inventory started at `feat/appimage-ci` / `5900ac8`. No new worktree was created.
Every registered worktree uses `/home/rapper/projects/kara/.git` as its common
directory. `kara-main`, `kara-ci` and the other linked directories are not
independent repositories. Removing or manually moving the original `.git` would
break them. The protected original remains on its existing rescue branch.

No merge, push, branch deletion, reset, force removal, Git-store relocation or
manual worktree move occurred. Non-Kara projects and existing backups were untouched.

## Complete worktree inventory before cleanup

All paths below are exact. "Ignored" counts regular/symlink file entries reported
by Git; byte totals in the machine inventory are logical sizes, not disk allocation.
All trees except the protected rescue and this CI repair were tracked/untracked
clean. All worktree reports were tracked; their hashes are in the internal inventory.

| Path | Branch | HEAD before repair | In main | Ignored files | Disposition |
|---|---|---|---|---:|---|
| `/home/rapper/projects/kara` | `rescue/shared-checkout-before-main-promotion-20261019T140759555033Z` | `b744b6b` | yes | 15,607 | Keep |
| `/home/rapper/projects/kara-audit` | `audit/engineering-quality` | `33239ee` | no | 3 | Keep |
| `/home/rapper/projects/kara-backup-restore` | `feat/backup-restore` | `22d0b30` | yes | 2 | Removed safely |
| `/home/rapper/projects/kara-calendar` | `feat/calendar-month` | `2ec9ed3` | yes | 690 | Keep |
| `/home/rapper/projects/kara-ci` | `feat/appimage-ci` | `5900ac8` | no | 4 | Keep |
| `/home/rapper/projects/kara-main` | `main` | `7514dfc` | yes | 2 | Keep |
| `/home/rapper/projects/kara-packaging` | `feat/release-packaging` | `199d69e` | no | 9 | Keep |
| `/home/rapper/projects/kara-performance` | `fix/performance-readiness` | `7514dfc` | yes | 2 | Removed safely |
| `/home/rapper/projects/kara-sidebar-polish` | `feat/sidebar-selector-polish` | `b744b6b` | yes | 0 | Removed safely |
| `/home/rapper/projects/kara-ui-polish` | `feat/tasks-ui-polish` | `716cf96` | yes | 6,512 | Keep |
| `/tmp/kara-matrix-feedback` | `feat/matrix-dnd-feedback` | `1daeb9d` | yes | 2 | Removed safely |
| `/tmp/kara-matrix-integration` | `integration/matrix-feedback` | `7259a39` | yes | 2 | Removed safely |

## Retained worktrees

- `/home/rapper/projects/kara`: Protected original rescue: 8 modified tracked files, 2 untracked Rust modules, 15,607 ignored build files; shared Git store and agent/shell working directories. Never overwrite.
- `/home/rapper/projects/kara-audit`: Unique unmerged documentation commit 33239ee, report and ignored metadata. Preserve.
- `/home/rapper/projects/kara-calendar`: Commit merged; 690 ignored files, 356,827,018 logical bytes of build artifacts not archived/approved for disposal. Retain for evidence review.
- `/home/rapper/projects/kara-ci`: Current CI repair branch; unmerged work and internal retirement archives. Keep active until owner-approved integration and evidence disposition.
- `/home/rapper/projects/kara-main`: Canonical clean main at 7514dfc. Keep active; no checkout or source changes.
- `/home/rapper/projects/kara-packaging`: Unmerged packaging commit 199d69e and ignored AppImage/checksum/build evidence. Preserve.
- `/home/rapper/projects/kara-ui-polish`: Commit merged; 6,512 ignored files, 2,566,114,949 logical bytes of build artifacts and historical Luna ownership. Retain; ownership cannot be proven from process names alone.

All retained worktrees passed `git rev-parse --is-inside-work-tree` and Git status
reads after cleanup. Main and packaging remain clean at their original commits.
Root rescue hashes, index and HEAD were compared before/after and matched exactly.

## Removal safeguards and recovery

For each of the five removed paths: HEAD was an ancestor of main; branch and
HEAD matched the saved inventory; no modified/untracked files, worktree lock or
active process reference was found. Every tracked file was checked against its
Git blob and a SHA-256 manifest. Tracked sources/reports remain reachable through
main history and their unchanged branch references.

All ignored files (eight generated desktop/AppStream files in total) were copied
into independent tar archives and read back/hash-verified. XML attribute ordering
made several outputs byte-distinct, so they were preserved rather than assumed
redundant. The sidebar candidate had no ignored files. Conditions and hashes were
repeated immediately before each plain `git worktree remove <exact-path>`.

Archives and complete before/after JSON inventories are under:

```text
/home/rapper/projects/kara-ci/packaging/work/retired-worktrees/
```

This is an existing-project internal ignored evidence location, not another
project/worktree. It is durable under `/home`; it is **not committed**. Preserve
it before retiring or relocating the CI worktree. Existing protected backups
were not repurposed or modified.

| Retired directory basename | Archived ignored files | ignored.tar SHA-256 |
|---|---:|---|
| `kara-sidebar-polish` | 0 | `84ff92691f909a05b224e1c56abb4864f01b4f8e3c854e4bb4c7baf1d3f6d652` |
| `kara-performance` | 2 | `9ceb1a7f43dad70fb6391697e2838d0970d73cceb33f895ac68c2e8c56aa9fa5` |
| `kara-backup-restore` | 2 | `deacb41e4ca12e0fabb646ebc6f3342b175982f98afdd6026402345e045e6919` |
| `kara-matrix-feedback` | 2 | `893170fb159ecce7e246ae190f4f474592b14dbcecd07bc8ea630a85b2b1e15d` |
| `kara-matrix-integration` | 2 | `1c738ceb491a1d75158a55ae528fb9182405ff5b9f5644e21d7bcc8fae00cd21` |

Each subdirectory contains `ignored.tar` and `manifest.json` (branch, HEAD,
tracked blob/file hashes and ignored file hashes). `inventory-before.json`,
`inventory-after.json`, `retirement-candidates.json` and `removed.json` provide
complete file lists and removal records. Inventory hashes:

- `inventory-before.json`: `3fe0208fd1be2743f48fdc388e4f254353a79b431f633c086d68d086c8e4984f`
- `inventory-after.json`: `552e886bc787f223e28ba2fbd4e561690490975000a5448f419d1325caf68c2e`
- `removed.json`: `56b07116a330ed5e2a1fd2c5b459510bf247cb8f95b6fee430257c5e98dedee3`

Recovery: use `git worktree add <new-path> <preserved-branch>` only when actually
needed. Verify the saved archive/manifest hashes, then restore only the archived
generated files at their relative paths. Never restore a stale `.git` link file;
Git must register the new worktree. No old worktree administrative links were
copied into the archives.

## Branch inventory (all preserved)

This is the pre-repair snapshot. Only `feat/appimage-ci` advances for the repair.
Cached tracking refs were inspected without fetch; they do not prove current
remote main state. Cached main was ahead of origin/main by 23 commits; cached
origin/feat/appimage-ci was 5900ac8.

```text
refs/heads/audit/engineering-quality 33239eebb7eaa4876937f4ea9548a14f223b02a1
refs/heads/backup/eisenhower-before-consolidation-20261018T230210Z 92508f2e99f44e44ba31bcb1b5d80d44c88c0a41
refs/heads/backup/main-before-consolidation-20261018T230210Z 72da937d7c0dc4a55ee00fe1f52b3160e4cb8e00
refs/heads/feat/appimage-ci 5900ac8981c7f9d51c817d405cd92468968a7808 refs/remotes/origin/feat/appimage-ci
refs/heads/feat/backup-restore 22d0b30de3a1ed423e942c20a602c9f7067f2540
refs/heads/feat/calendar-month 2ec9ed35b63b433ec2c0d707ee908c758179d0ed
refs/heads/feat/eisenhower 92508f2e99f44e44ba31bcb1b5d80d44c88c0a41
refs/heads/feat/matrix-dnd-feedback 1daeb9de2c64c3fbdfb26ce1aa6d5a5472a297ad
refs/heads/feat/release-packaging 199d69ec717eaac996950ebc2140f326088efbd1
refs/heads/feat/sidebar-selector-polish b744b6bd4312bf9d16654f0a5a0b9b4df1268ec5
refs/heads/feat/tasks-ui-polish 716cf96f5833424d7e2488dd51e6df3a216896d1
refs/heads/fix/performance-readiness 7514dfc460e0d00bb0fba1e321d55115e990f3b1
refs/heads/integration/matrix-feedback 7259a39d37592ce6cb1280cb9004fdb0d7d53920
refs/heads/main 7514dfc460e0d00bb0fba1e321d55115e990f3b1 refs/remotes/origin/main [ahead 23]
refs/heads/rescue/shared-checkout-before-main-promotion-20261019T140759555033Z b744b6bd4312bf9d16654f0a5a0b9b4df1268ec5
```

The only pre-existing commits outside main were audit `33239ee`, packaging
`199d69e` and CI `5900ac8` (including its packaging parent). Their worktrees and
branches remain. Other feature/backup/rescue branch tips are included in main.

## Activity and other directories

Host `/proc` scans checked working directories, executable paths, command-line
path references and open file descriptors immediately before retirement. No
process referenced a retired tree. Several Codex/Node processes and shells had
`/home/rapper/projects/kara` as cwd; the current task used `kara-ci`. Process
names do not reliably identify Luna versus Sol; the original and UI evidence
remain protected. No agents or processes were killed.

Protected ordinary backup/recovery directories retained:

- `/home/rapper/projects/kara-main-promotion-backup-20261019T140759555033Z`
- `/home/rapper/projects/kara-matrix-backup-20261018T224201134002Z`
- `/home/rapper/projects/kara-matrix-dnd-backup-20261019T040530Z`
- `/tmp/kara-matrix-dnd-preserved-20261019T040530Z`
- `/tmp/kara-promotion-validation-8kuq7e4t`
- `/tmp/kara-eisenhower-review`

Build/evidence resources retained; none is a linked worktree:

- `/var/tmp/kara-appimage-ci-build`
- `/var/tmp/kara-packaging-build`
- `/var/tmp/kara-packaging-podman`
- `/var/tmp/kara-packaging-podman-run`
- `/var/tmp/kara-matrix-integration-target`
- `/var/tmp/kara-performance-measurements`
- `/tmp/kara-tabler-target`
- `/tmp/kara-ui-polish-target`

Other Kara helper/patch/measurement files under `/tmp` were inventoried by name
and left untouched. `/home/rapper/Projects` does not exist now; only lowercase
`projects` is in use. No filesystem case aliases, ordinary folders or backups
were moved or deleted in this task.

## Recommended layout and owner approval boundary

For now, keep `/home/rapper/projects/kara-main` canonical and reuse it for future
milestones. Isolation is exceptional: use a designated ignored `.worktrees/`
location inside the canonical project, not another top-level `kara-*` directory.
Add the ignore rule when this policy is adopted; no new worktree/ignore change
was needed here. After successful integration, audit/archive any unique ignored
evidence and retire the temporary worktree normally. Keep branch refs as needed.

The eventual single permanent project root should be:

```text
/home/rapper/projects/kara/
  .git/                    existing shared store; never move manually
  <accepted main sources>  only after explicit rescue-preservation approval
  .worktrees/              short-lived isolated development when necessary
  .archives/               reviewed recovery/evidence, never silently discarded
```

**Do not implement this conversion yet.** Making the protected original the
normal main checkout changes its current rescue files and needs owner approval.
Proposed reviewed migration sequence:

1. Freeze agents; verify independent all-ref Git bundle and complete rescue file
   backup, including untracked files and a disposition for ignored artifacts.
2. With explicit approval, checkpoint rescue edits on its rescue branch without
   merging them, and preserve an independent recoverable rescue checkout.
3. Review unmerged audit/packaging/CI work and unique Calendar/UI build evidence.
   Use only `git worktree move/remove` for linked paths; no manual directory move.
4. Once main is no longer attached elsewhere and the original rescue is fully
   preserved, obtain approval to attach the original root to main using normal
   non-forced Git operations. Do not bypass any dirty/untracked-file refusal.
5. Adopt ignored internal worktree/archive locations and verified recovery paths.
   Existing external protected backups remain until separately reviewed.

No step in that migration was performed. Seven registered worktrees remain
(down from twelve); remaining retention is deliberate, not silent data loss.
