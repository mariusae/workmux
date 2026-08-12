---
title: Sapling worktrees
description: Use workmux with EdenFS-backed Sapling repositories
---

Workmux automatically detects EdenFS-backed Sapling repositories and manages
their linked working copies with `sl worktree`. Git repositories continue to
use `git worktree`; no configuration switch is required.

```bash
workmux add my-task
workmux list
workmux open my-task
workmux remove my-task
```

Use `-d` to prefix both the worktree label and workmux handle with today's
date, for example `workmux add -d my-task` creates `2026-08-12-my-task`.

For Sapling, the workmux handle is stored as the worktree label. `add` creates
the working copy at the requested base revision (or `.` by default), and
`rename` changes the label without moving the EdenFS mount point. Removal is
performed through `sl worktree remove`; workmux never recursively deletes an
EdenFS working copy.

The following workflows are supported: `add`, `list`, `open`, `close`,
`remove`, `rename` (label only), `path`, `setup`, `sync-files`, `resurrect`, and
agent dispatch/status tracking.

Branch-oriented operations remain Git-only: `merge`, `rebase`, `add --pr`,
`add --with-changes`, `rename --branch`, `remove --gone`, and `status --git`.
Workmux reports a capability error for these commands instead of invoking Git
inside a Sapling repository.

`sl worktree` currently requires an EdenFS-backed repository. If Sapling
reports that worktrees are unsupported, configure or migrate the checkout to
EdenFS before using workmux.
