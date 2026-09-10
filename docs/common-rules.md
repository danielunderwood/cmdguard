# Common Rules Recipes

Copy-paste policy snippets for tools not covered by the base policy set. Drop these into `~/.config/cmdguard/policies/` or into a project-local `.cmdguard/` directory.

Every file must start with:

```rego
package cmdguard

import rego.v1
```

---

## PostgreSQL (psql)

Restrict psql to localhost connections only.

```rego
package cmdguard

import rego.v1

# Allow psql only when connecting to localhost
rules["allowed_psql"] := allow("psql to localhost") if {
    input.command[0] == "psql"
    input.command[1] == "-h"
    input.command[2] == "localhost"
}
```

More permissive: allow psql to any host but deny destructive flags.

```rego
package cmdguard

import rego.v1

# Allow psql read-only use (no -c with write statements checked elsewhere)
rules["allow_psql"] := allow("psql allowed") if {
    input.command[0] == "psql"
}

# Deny psql -c with obviously destructive SQL
rules["deny_psql_drop"] := deny("DROP statements blocked in psql") if {
    input.command[0] == "psql"
    some i
    input.command[i] == "-c"
    regex.match(`(?i)\b(DROP|TRUNCATE|DELETE\s+FROM)\b`, input.command[i + 1])
}
```

---

## Curl URLs

Allow curl only when every URL written on the command line targets a local
service by contributing a regular expression to the built-in set:

```rego
package cmdguard

import rego.v1

allowed_curl_patterns contains `^http://localhost:3000($|/)`
```

Put this in a file cmdguard loads. With the synced base layout that is
`~/.config/cmdguard/policies/custom.rego` or any `*.rego` beside it; with a flat
`--policy-dir DIR` (`cmdguard eval`/`cmdguard test` against a checkout's
`config/`) only the top-level `*.rego` files of `DIR` are loaded, so the pattern
has to sit directly in that directory.

Set membership matches the data: each entry is a regex, and `contains` lets
multiple policy files contribute patterns without replacing a list or storing
dummy boolean values. Each pattern is anchored at the start before it is
matched -- `regex.match` otherwise searches anywhere in the URL -- so it must
describe the URL from its first character, and a host pattern should end with
`($|/)` so that `http://localhost:3000.evil.example/` cannot match it.

The base policy requires at least one declared URL and checks every bare URL and
repeated `--url` value. It keeps asking when a URL does not match, when the
command carries a flag cmdguard does not model, when a wrapper or `VAR=value`
prefix could redirect the request, when an option such as `-L`, `--config`,
`--connect-to`, `--resolve`, a proxy, `--doh-url` or a Unix socket can remap the
destination, when an option writes or reads a local file (`-o`, `-O`, `-T`,
`-c`, `-b`, `--trace`, `--netrc-file`, `--cacert`, an `@file` value on
`-d`/`-H`/`-F`/`-w`), and when a URL token contains something the shell or curl
expands before the request (`$(...)`, backticks, a leading `~`, `{a,b}`,
`[1-9]`, `*`). Higher-priority safety asks such as shell output redirection
still win.

This checks URLs present in the command only. Curl can also load `~/.curlrc`
implicitly. Requiring `-q` or `--disable` as the first curl option disables that
file when the policy must constrain the runtime connection rather than only the
declared URLs.

---

## Jira CLI

Allow read-only Jira operations. The [go-jira](https://github.com/ankitpokhrel/jira-cli) CLI uses positional subcommands.

```rego
package cmdguard

import rego.v1

# Simple allow-list via first-arg table
allowed_with_args["jira"] := {"help", "me", "move"}

# Allow --help on any jira command
rules["jira_command_help"] := allow("jira command help") if {
    input.binary_name == "jira"
    input.parsed_flags.help
}

# Read-only compound subcommands
rules["jira_issue_view"] := allow("Jira issue view") if {
    input.binary_name == "jira"
    input.positional.args[0].raw == "issue"
    input.positional.args[1].raw in {"view", "list"}
}

rules["jira_epic_read"] := allow("Jira epic read") if {
    input.binary_name == "jira"
    input.positional.args[0].raw == "epic"
    input.positional.args[1].raw in {"list", "view"}
}

rules["jira_project_list"] := allow("Jira project list") if {
    input.binary_name == "jira"
    input.positional.args[0].raw == "project"
    input.positional.args[1].raw == "list"
}
```

---

## Nix

Allow `nix build`, `nix flake`, and related read-only operations.

```rego
package cmdguard

import rego.v1

is_nix if input.command[0] == "nix"

is_nix_flake if {
    is_nix
    input.command[1] == "flake"
}

rules["allowed_nix"] := allow("Allowed nix command") if {
    is_nix
    input.command[1] in {"build", "develop", "run", "shell", "version"}
}

rules["allowed_flake"] := allow("Allowed flake command") if {
    is_nix_flake
    input.command[2] in {"check", "info", "show", "update", "lock"}
}

# nh (Nix Helper)
rules["allowed_nh"] := allow("Allowed nh command") if {
    input.command[0] == "nh"
    input.command[1] == "search"
}
```

---

## Mise

[Mise](https://mise.jdx.dev/) task runner and version manager.

```rego
package cmdguard

import rego.v1

allowed_mise_commands := {
    "build",
    "check",
    "env",
    "install",
    "run",
    "t",
    "tasks",
    "test",
    "version",
}

rules["allowed_mise"] := allow("Allowed mise command") if {
    input.command[0] == "mise"
    input.command[1] in allowed_mise_commands
}
```

More restrictive: only allow specific task names.

```rego
package cmdguard

import rego.v1

# Allow only known mise task names
rules["allowed_mise_tasks"] := allow("Allowed mise task") if {
    input.command[0] == "mise"
    input.command[1] in {"run", "t"}
    input.command[2] in {"build", "test", "lint", "fmt"}
}
```

---

## Make

Make runs arbitrary targets defined in Makefile. Because any target can execute arbitrary shell code, `make` rules are best used as **project-local** policies in `.cmdguard/`.

**Allow everything** (put in `.cmdguard/make.rego`):

```rego
package cmdguard

import rego.v1

# Allow all make invocations in this project
rules["allow_make"] := allow("Make allowed in this project") if {
    input.binary_name == "make"
}
```

**Allow specific targets only** (put in `.cmdguard/make.rego`):

```rego
package cmdguard

import rego.v1

# Only allow known targets
allowed_with_args["make"] := {"build", "test", "clean", "lint", "fmt"}
```

**Global: ask for confirmation on all make commands** (put in `~/.config/cmdguard/policies/`):

```rego
package cmdguard

import rego.v1

rules["make_ask"] := ask("make runs Makefile targets - confirm") if {
    input.binary_name == "make"
}
```

---

## Cloud CLIs

### AWS CLI

```rego
package cmdguard

import rego.v1

# Allow read-only AWS commands
rules["aws_readonly"] := allow("AWS read-only command") if {
    input.command[0] == "aws"
    input.command[2] in {
        "describe-instances",
        "get-object",
        "list-buckets",
        "list-functions",
        "list-objects",
        "list-stacks",
        "list-tables",
    }
}

# Deny destructive AWS commands
rules["aws_deny_delete"] := deny("AWS delete operations blocked") if {
    input.command[0] == "aws"
    startswith(input.command[2], "delete-")
}

rules["aws_deny_terminate"] := deny("AWS terminate operations blocked") if {
    input.command[0] == "aws"
    startswith(input.command[2], "terminate-")
}
```

### Google Cloud CLI

```rego
package cmdguard

import rego.v1

# Allow gcloud read-only operations
rules["gcloud_readonly"] := allow("gcloud read-only command") if {
    input.command[0] == "gcloud"
    some i
    input.command[i] in {"describe", "list", "get-iam-policy"}
}

# Deny gcloud delete
rules["gcloud_deny_delete"] := deny("gcloud delete blocked") if {
    input.command[0] == "gcloud"
    some i
    input.command[i] == "delete"
}
```

### Terraform

```rego
package cmdguard

import rego.v1

# Allow terraform plan and read-only commands
allowed_with_args["terraform"] := {"fmt", "init", "plan", "validate", "version"}
allowed_with_args["tofu"] := {"fmt", "init", "plan", "validate", "version"}

# Ask before apply/destroy
rules["terraform_apply_ask"] := ask("terraform apply modifies infrastructure") if {
    input.command[0] in {"terraform", "tofu"}
    input.command[1] == "apply"
}

rules["terraform_destroy_deny"] := deny("terraform destroy blocked") if {
    input.command[0] in {"terraform", "tofu"}
    input.command[1] == "destroy"
}
```

---

## OpenSSL

```rego
package cmdguard

import rego.v1

# Allow safe openssl subcommands
allowed_with_args["openssl"] := {
    "dgst",
    "enc",
    "rand",
    "req",
    "s_client",
    "version",
    "x509",
}
```

---

## OPA

Allow [Open Policy Agent](https://www.openpolicyagent.org/) CLI commands.

```rego
package cmdguard

import rego.v1

rules["safe_opa"] := allow("Allowed opa command") if {
    input.command[0] == "opa"
    input.command[1] in {
        "eval",
        "exec",
        "fmt",
        "help",
        "parse",
        "test",
        "version",
    }
}
```

---

## Nickel

Allow [Nickel](https://nickel-lang.org/) evaluation.

```rego
package cmdguard

import rego.v1

rules["nickel_eval"] := allow("Nickel eval allowed") if {
    input.binary_name == "nickel"
    input.positional.args[0].raw in {"eval", "export", "format", "typecheck"}
}
```

---

## Tips

**Combine recipes freely.** Each file contributes rules to the same `cmdguard` package. Drop multiple recipe files into the same directory and they merge automatically.

**Use exclusion tables to tighten base rules.** If the base set allows a subcommand you want to block, add an exclusion in your user policies instead of rewriting the base file:

```rego
package cmdguard

import rego.v1

# Block cargo publish (allowed by base rust.rego)
denied_subcommands["cargo"] := {"publish"}
```

**Use `cmdguard eval` to test your rules:**

```bash
cmdguard eval "psql -h localhost mydb"
cmdguard eval "make build"
cmdguard eval "aws s3 list-buckets"
```

**Use `cmdguard status` to see what is loaded:**

```bash
cmdguard status
```

---

## Deferring to the agent instead of prompting

Use `defer(reason)` when cmdguard recognizes a command but you'd rather let
the agent's own permission flow decide than hard-prompt:

```rego
# Let the agent weigh terraform apply in context instead of always
# prompting. In silent defer_mode this emits no hook output.
rules["defer_terraform_apply"] := defer("terraform apply deferred to agent") if {
    input.binary_name == "terraform"
    input.subcommand == "apply"
}
```

`defer` has the lowest priority (10), so any matching `allow`/`ask`/`deny`
rule wins over it. In a compound command, the most-restrictive segment
decides: `deny > ask > defer > allow`.

In Claude Code, `defer_mode = "prompt"` turns a winning defer into an explicit
prompt. Codex `PreToolUse` cannot force an approval prompt, so the same setting
produces no hook decision unless Codex independently raises a
`PermissionRequest`.
