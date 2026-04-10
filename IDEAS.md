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
