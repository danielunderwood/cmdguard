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

- Transient/scratch trust zone for paths
  - Currently TrustZone is one of project/user/system/unknown. Anything
    outside ~/.local/bin etc. and the project root falls into "unknown",
    which is too coarse: `/tmp/foo`, `/var/tmp/foo`, `$TMPDIR/foo`, and
    `~/.cache/foo` get the same treatment as `/etc/passwd`.
  - A `TrustZone::Transient` (or similar) variant would let rules say "rm
    -rf in /tmp is fine, but ask for arbitrary 'unknown' paths." Today
    deny_rm_outside_project / ask_rm_outside_project can't tell those
    apart, so the rule had to be downgraded to ask everywhere.
  - Default transient roots: `/tmp`, `/var/tmp`, `$TMPDIR`, `~/.cache`,
    plus a way for users to extend in policy or config.
  - Path classifier lives in src/resolver.rs (TrustZonePaths) and the
    positional path classifier in src/command_parser.rs around the
    `trust_zone` block — both would need updating.

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

- First-run experience
  - `cmdguard init` that generates a starter `custom.rego` with commented
    examples, similar to the template emitted by `base sync`.
  - Detect an empty `policies/` directory and offer a guided setup
    (permissive / restrictive / custom profiles).

- Testing improvements
  - `cmdguard test --watch` to rerun the policy suite when `.rego` files
    change.
  - `cmdguard check` to report rule coverage — which rules have/don't
    have test cases.

- TOML simple mode
  - A `cmdguard.toml` config surface for the 80% case (allow-lists, simple
    deny rules) that compiles to Rego internally at load time. Users who
    need anything beyond that (trust-zone checks, iteration over positional
    args, etc.) drop down to Rego.
  - Example:
    ```toml
    [git]
    allow = ["status", "log", "diff", "commit", "push"]
    deny_flags.push = ["force"]

    [cargo]
    allow = ["build", "test", "check", "fmt", "clippy", "run"]
    ```
  - Additive — doesn't replace Rego, just provides a friendlier entry point.

- URL parsing for curl/wget rules
  - Extract domain/scheme/path from URL-typed positional args so rules can
    write `input.positional.url[0].domain == "api.github.com"` instead of
    matching against regexes.
  - Rust-side parsing with the `url` crate is probably the right approach
    (reliable, fast, already a transitive dep). Alternatives: a Nickel
    post-parse transform, or a Rego helper.

- Language choice rationale (notes for future readers)
  - Rego (via regorus) + Nickel were chosen after evaluating Lua, Rhai,
    Wasm, KCL, and a Nickel-only design.
  - Rego stays for policy: declarative model, partial-rule merging, and
    fail-loud semantics fit allow/deny decisions well.
  - Nickel handles command schemas (rarely user-facing) without imposing
    its full programming model on rule authors.
  - Lua: more readable for imperative logic but loses automatic multi-file
    rule merging; would need significant Rust glue to reimplement.
  - Wasm: too heavy for the &lt;100 ms per-call budget. KCL: too new/niche.
    Rhai: Rust-only community, smaller ecosystem.
  - The TOML simple mode (above) is the better path to accessibility
    without an engine rewrite.
