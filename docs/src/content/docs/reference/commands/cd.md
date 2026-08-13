---
title: "cd"
description: Change directory to a worktree
---

Changes the current shell's directory to the selected main or linked worktree.

```bash
workmux cd <name>
```

## Arguments

- `<name>`: Worktree handle or Sapling worktree label.

## Shell integration

Changing the parent shell's directory requires the wrapper emitted by
`workmux completions`. Follow the [shell completion installation
instructions](/guide/installation/#shell-completions) before using this
command.

Without the wrapper, the `workmux` binary prints the resolved path instead of
changing the calling shell's directory.

## Example

```bash
workmux cd user-auth
```
