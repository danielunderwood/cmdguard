# Ideas

Things that popped into my head, but aren't yet implemented.

- Run as daemon
  - Slightly improved performance
  - Could add interactive rules
  - Could add temporary rule for X amount of time

- Generate command definitions from help/man pages
  - Parse `--help` output to extract flags, their types, and short/long forms
  - Parse man pages for more detailed info (positional args, subcommands)
  - Could use LLM to interpret ambiguous help text
  - Output Nickel command definition format
  - Challenges:
    - Help output formats vary wildly between tools
    - Some tools have subcommands with their own flags (`git commit --help`)
    - Detecting flag types (boolean vs with_arg) from description
    - Identifying positional args and their semantics (path vs string)
  - Could be a separate CLI command: `cmdguard generate <cmd>`
  - Or interactive: run command, observe actual usage patterns

- Nickel post-parse transforms (more flexible than claim_pattern)
  - Currently we have `claim_pattern` for regex-based flag capture (e.g., `-30` → `lines: "30"`)
  - For more complex cases, allow Nickel functions to transform parse results:
    ```nickel
    head = {
      flags = { ... },
      post_parse = fun result =>
        # result = { flags, unknown_flags, positional }
        let numeric = result.unknown_flags
          |> std.array.filter (std.string.is_match "^-\\d+$")
          |> std.array.first
        in
        if numeric != null then
          result & { flags = result.flags & { lines = numeric |> std.string.substring 1 } }
        else
          result
    }
    ```
  - Would require calling back into Nickel during parsing
  - More powerful but adds complexity and potential performance cost
  - Consider if claim_pattern proves insufficient for real-world use cases

- Command aliases (python/python3, pip/pip3, vi/vim, node/nodejs, ...)
  - Currently builtins.ncl duplicates the full definition for each spelling,
    which drifts and bloats the file.
  - Proposed: a Nickel `aliases = { python3 = "python", pip3 = "pip", ... }`
    table. At load time, alias keys resolve to the canonical CommandDef so
    parsing is identical for every spelling.
  - Rego rules also need to normalize. Two options:
    - Expose a new `input.canonical_binary_name` field, populated from the
      same alias table during input construction. Rules write
      `input.canonical_binary_name == "python"` and match all aliases. One
      contract, single source of truth, but rule authors have a new field
      to learn and existing rules need a migration pass.
    - Sprinkle `input.binary_name in {"python", "python3"}` in each rule.
      Less new machinery but loses the point of having an alias table.
    - Lean toward the canonical_binary_name approach.
  - Keep `input.binary_name` as the literal typed name so eval output stays
    honest about what the user actually invoked.
  - Start with explicit aliases only. Auto-aliasing on PATH symlinks looks
    tempting but gets surprising in nix/containers where everything is a
    symlink chain.
