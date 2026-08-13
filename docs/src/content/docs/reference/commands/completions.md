---
title: "completions"
description: Generate shell completion scripts for bash, zsh, or fish
---

Generates shell initialization for the specified shell. It provides tab-completion for commands and dynamic worktree suggestions. For Bash, Zsh, and Fish it also defines the `workmux` function required by `workmux cd`.

```bash
workmux completions <shell>
```

## Arguments

- `<shell>`: Shell type: `bash`, `zsh`, or `fish`.

## Examples

```bash
# Generate completions for zsh
workmux completions zsh
```

See [Installation - Shell completions](/guide/installation/#shell-completions) for setup instructions.
