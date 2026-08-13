# Dynamic worktree handle completion (directory names)
# Used for open/remove/merge/path/close - repo-scoped lifecycle commands
function __workmux_handles
    command workmux _complete-handles 2>/dev/null
end

# All worktrees, including the main checkout, for navigation commands
function __workmux_worktrees
    command workmux _complete-worktrees 2>/dev/null
end

# Dynamic agent target completion (local handles + cross-project agents)
# Used for send/capture/status/wait/run - agent communication commands
function __workmux_agent_targets
    command workmux _complete-agent-targets 2>/dev/null
end

# Dynamic git branch completion for add command
function __workmux_git_branches
    command workmux _complete-git-branches 2>/dev/null
end

# Lifecycle commands: local handles only
complete -c workmux -n '__fish_seen_subcommand_from open remove rm rename path merge rebase close' -f -a '(__workmux_handles)'
# Navigation commands: main checkout + linked worktrees
complete -c workmux -n '__fish_seen_subcommand_from cd' -f -a '(__workmux_worktrees)'
# Agent commands: local + cross-project targets
complete -c workmux -n '__fish_seen_subcommand_from send capture status wait run' -f -a '(__workmux_agent_targets)'
# Add command: git branches
complete -c workmux -n '__fish_seen_subcommand_from add' -f -a '(__workmux_git_branches)'

# A child process cannot change Fish's working directory. Intercept `workmux cd`,
# ask the binary to resolve the destination, then invoke Fish's cd builtin.
function workmux --description 'Manage worktrees and terminal multiplexer targets'
    if test (count $argv) -gt 0; and test "$argv[1]" = cd
        set -l destination (command workmux $argv)
        set -l command_status $status
        if test $command_status -ne 0
            return $command_status
        end
        if test (count $destination) -ne 1
            echo 'workmux cd: expected exactly one destination path' >&2
            return 1
        end
        builtin cd -- "$destination"
    else
        command workmux $argv
    end
end
