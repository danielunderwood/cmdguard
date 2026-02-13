# Example Nickel configuration for claude-permissions
#
# This file shows how to define custom wrapper extractors.
# To use: copy to ~/.config/claude-permissions/commands.ncl
#
# Wrapper extract functions take an array of tokens and return either:
#   - { remaining = [...], wrapper_name = "..." } to extract a wrapper
#   - null if this isn't a wrapper invocation
let stub_subcommands = fun subcommands =>
  let entries = std.array.map (fun command => { field = command, value = {} }) subcommands in
  std.record.from_array entries
in
let extract_after = fun tokens arg =>
  let len = std.array.length tokens in
  let command = std.array.at 0 tokens in
  if len >= 4 && std.array.at 1 tokens == arg && std.array.at 2 tokens == "--" then
    { remaining = std.array.slice 3 len tokens, wrapper_name = "%{command} %{arg}" }
  else if len >= 3 && std.array.at 1 tokens == arg then
    { remaining = std.array.slice 2 len tokens, wrapper_name = "%{command} %{arg}" }
  else
    null
in
{
  wrappers = {
    xargs = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        let command = std.array.at 0 tokens in
        { remaining = std.array.slice 1 len tokens, wrapper_name = command }
    },
    timeout = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        let command = std.array.at 0 tokens in
        { remaining = std.array.slice 2 len tokens, wrapper_name = command }
    },

    uv = {
      extract = fun tokens => extract_after tokens "run"
    },
    mise = {
      extract = fun tokens => extract_after tokens "exec"
    },

    # Example: Flatpak wrapper
    # Usage: flatpak run <app-id> [command]
    flatpak = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        if len >= 4 && std.array.at 1 tokens == "run" then
          # Skip: flatpak run <app-id> -> remaining is the command
          { remaining = std.array.slice 3 len tokens, wrapper_name = "flatpak run" }
        else
          null
    },

    # Example: SSH wrapper (simplified - skips first arg as host)
    # Usage: ssh user@host <command>
    ssh = {
      extract = fun tokens =>
        let len = std.array.length tokens in
        # Simple version: assume ssh <host> <command...>
        if len >= 3 then
          { remaining = std.array.slice 2 len tokens, wrapper_name = "ssh" }
        else
          null
    },

    # Example: Override built-in sudo with custom logic
    # This shows how to intercept before the built-in
    # sudo = {
    #   extract = fun tokens =>
    #     let len = std.array.length tokens in
    #     # ... custom sudo extraction logic ...
    #     null  # or a result record
    # },
  },

  # Command definitions can also be customized here
  # These supplement the built-in definitions
  commands = {
    cargo.subcommands =
      stub_subcommands [
        "bench",
        "build",
        "check",
        "clean",
        "clippy",
        "config",
        "doc",
        "fix",
        "fmt",
        "generate-lockfile",
        "help",
        "info",
        "list",
        "locate-project",
        "metadata",
        "pkgid",
        "run",
        "report",
        "rustc",
        "rustdoc",
        "search",
        "test",
        "version"
      ],
    git.subcommands = {
      rev-parse = {
        flags = {},
        positional = [
          { name = "revision", type = "string", variadic = false }
        ]
      },
      show = {},
      ls-tree = {},
      fetch = {},
      worktree = {
        positional = [
          { name = "subcommand", type = "string", variadic = "false" }
        ]
      }
    },
    curl = {
      flags = {
        silent = { short = ["-s"], type = "boolean" },
        user = { short = ["-u"], type = "with_arg" },
        header = { short = ["-H"], type = "repeatable" },
        format = { short = "-w", type = "with_arg" }
      },
      positional = [
        { name = "url", type = "string", variadic = false, last = true }
      ]
    },
    python = {
      flags = {
        module = { short = ["-m"], type = "with_arg" }
      },
      positional = [
        { name = "file", type = "string", variadic = false }
      ]
    }
  },

  # Global parsing defaults (optional - these are the defaults)
  defaults = {
    combine_short_flags = true,
    double_dash_ends_flags = true,
  },
}
