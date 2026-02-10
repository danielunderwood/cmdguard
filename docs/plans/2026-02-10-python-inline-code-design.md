# Python Inline Code Handling

**Date:** 2026-02-10
**Status:** Draft

## Problem

Commands like `python -c 'import x; print(x)'` are common in Claude Code workflows, typically for:
- Inspecting documentation (`print(foo.__doc__)`)
- Checking function signatures (`inspect.signature(func)`)
- Quick data transformations

Currently these trigger permission prompts, hurting workflow. We want to:
1. Allow safe inspection code without prompts
2. Sandbox execution code to prevent unintended side effects
3. Maintain security (no secret exfiltration via network, no destructive operations)

## Design Overview

```
python -c '...'
       │
       ▼
┌─────────────────┐
│  Parse & Classify│
│  (tree-sitter)  │
└────────┬────────┘
         │
    ┌────┴────┐
    │         │
Inspection  Execution
   Mode       Mode
    │         │
    ▼         ▼
 Static    Sandboxed
Analysis   Execution
    │         │
    ▼         ▼
 Native    Network-
  Exec     blocked
```

### Two Modes

| Mode | Characteristics | Handling |
|------|-----------------|----------|
| **Inspection** | Read-only operations, known imports | Static analysis → allow native execution |
| **Execution** | Side effects, network, file writes | Run in sandbox with restricted capabilities |

## Mode 1: Inspection (Static Analysis)

### Goal

Allow code like:
```python
python -c 'import pandas; print(pandas.DataFrame.__doc__)'
python -c 'import inspect; print(inspect.signature(my_func))'
```

### Safety Criteria

1. **Imports are known dependencies**
   - All imports must exist in `pyproject.toml`, `requirements.txt`, or stdlib
   - Prevents arbitrary module loading

2. **Operations are read-only**
   - Attribute access: `foo.bar`, `foo.__doc__`
   - Safe builtins: `print`, `type`, `dir`, `help`, `repr`, `str`, `len`
   - Inspect module: `inspect.signature`, `inspect.getsource`, etc.
   - No calls to methods that mutate or have side effects

3. **No dangerous patterns**
   - No `eval`, `exec`, `compile`, `__import__`
   - No `open()`, `file()`, file operations
   - No `subprocess`, `os.system`, `os.popen`
   - No network imports (`socket`, `urllib`, `requests.post`)

### Implementation Options

#### Option A: Tree-sitter Queries (Recommended)

Tree-sitter has a [query language](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/index.html)
that provides declarative pattern matching without raw AST traversal:

```scheme
; Match dangerous imports
(import_statement
  name: (dotted_name) @import
  (#match? @import "^(os|subprocess|socket|shutil)$"))

; Match from X import Y
(import_from_statement
  module_name: (dotted_name) @module
  (#match? @module "^(os|subprocess)$"))

; Match eval/exec calls
(call
  function: (identifier) @func
  (#match? @func "^(eval|exec|compile|__import__)$"))

; Match open() calls
(call
  function: (identifier) @func
  (#eq? @func "open"))

; Match method calls like os.system()
(call
  function: (attribute
    object: (identifier) @obj
    attribute: (identifier) @method)
  (#eq? @obj "os")
  (#match? @method "^(system|popen|exec)"))
```

**Pros:**
- Declarative patterns (more maintainable than Rust code)
- tree-sitter already a dependency
- Query predicates support regex matching

**Cons:**
- Query language has learning curve
- Some complex patterns may still need Rust code

#### Option B: Semgrep

[Semgrep](https://semgrep.dev/) is a pattern-based static analysis tool with extensive pre-built rules.

```yaml
rules:
  - id: dangerous-python-patterns
    patterns:
      - pattern-either:
          - pattern: import subprocess
          - pattern: import os
          - pattern: from os import $X
          - pattern: eval(...)
          - pattern: exec(...)
          - pattern: open($FILE, "w")
    message: "Potentially dangerous operation"
    severity: WARNING
    languages: [python]
```

**Pros:**
- Pre-built security rules
- High-level pattern syntax
- Extensive Python coverage

**Cons:**
- No Rust bindings (would need to shell out to CLI)
- Heavier dependency (~50MB)
- External tool management

#### Option C: Raw Tree-sitter AST

Manual AST traversal in Rust:

```rust
// Pseudo-code
fn is_inspection_safe(code: &str, project_deps: &HashSet<String>) -> Result<bool, String> {
    let tree = parse_python(code);

    // Check all imports
    for import in tree.imports() {
        let module = import.root_module();
        if !project_deps.contains(module) && !is_stdlib(module) {
            return Err(format!("Unknown import: {}", module));
        }
    }

    // Check all calls
    for call in tree.calls() {
        if !is_readonly_call(call) {
            return Err(format!("Non-readonly call: {}", call));
        }
    }

    Ok(true)
}
```

**Pros:**
- Full control
- No new dependencies

**Cons:**
- More code to write and maintain
- Pattern changes require Rust changes

### Readonly Call Allowlist

```
# Builtins (safe)
print, type, dir, help, repr, str, len, isinstance, issubclass,
hasattr, getattr, id, hash, vars, sorted, reversed, enumerate, zip, map, filter

# Attribute access patterns (safe)
__doc__, __name__, __module__, __qualname__, __annotations__,
__class__, __bases__, __mro__, __dict__ (read)

# Inspect module (safe)
inspect.signature, inspect.getsource, inspect.getfile, inspect.getmembers,
inspect.isfunction, inspect.isclass, inspect.ismethod, ...

# Explicitly unsafe
open, file, eval, exec, compile, __import__, input,
setattr, delattr, globals, locals (mutation)
```

## Mode 2: Execution (Sandboxed)

### Goal

Run code that does actual work, but restrict dangerous capabilities:
- Block network access
- Restrict filesystem (read-only or temp-only writes)
- Handle subprocess limitations

### Sandbox Options

#### Option A: WASM (Pyodide)

**Architecture:**
```
claude-permissions daemon
        │
        ▼
┌─────────────────────────────┐
│  Warm Pyodide Instance      │
│  - CPython 3.11 in WASM     │
│  - Pre-loaded packages      │
│  - Virtual filesystem       │
└─────────────────────────────┘
```

**Startup times:**
- Cold: 3-7 seconds (load WASM, init Python, import packages)
- Warm: <50ms (eval in running instance)

**Pros:**
- True sandbox (WASM capability model)
- Cross-platform
- No network/filesystem by default

**Cons:**
- Not all packages available (C extensions need WASM compilation)
- No subprocess support (fundamental WASM limitation)
- Memory overhead for daemon

**Available packages:**
- stdlib (full)
- numpy, pandas, scipy, scikit-learn, matplotlib
- requests, beautifulsoup4, lxml
- sqlalchemy (pure Python parts)
- Many pure-Python packages via micropip

#### Option B: Native + Network Sandbox

**Linux:**
```bash
# Using unshare to create network namespace
unshare --net python -c '...'

# Or using seccomp to filter syscalls
# (requires seccomp-bpf filter compilation)
```

**macOS:**
```bash
# Using sandbox-exec (deprecated but functional)
sandbox-exec -p '(version 1)(deny network*)' python -c '...'
```

**Pros:**
- All user packages available
- Subprocess works (with inherited restrictions)
- Lower overhead than WASM

**Cons:**
- Platform-specific implementation
- sandbox-exec is undocumented/deprecated
- seccomp requires careful filter design

#### Option C: Hybrid

```
Execution mode code
        │
        ▼
┌─────────────────────┐
│ Can Pyodide handle? │
│ (check imports)     │
└──────────┬──────────┘
           │
     ┌─────┴─────┐
     │           │
    Yes          No
     │           │
     ▼           ▼
  Pyodide    Native +
  sandbox    network sandbox
```

### Subprocess Considerations

**In Pyodide:** Not supported. Code using subprocess will fail.

**In native sandbox:**
- Child processes inherit sandbox restrictions (namespace/seccomp)
- sandbox-exec restrictions apply to children
- Need to ensure no escape via subprocess

**Mitigation:**
- Static analysis can detect subprocess usage
- Route subprocess-using code to native sandbox (not Pyodide)
- Or ask user for subprocess code

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                   claude-permissions                         │
│                                                             │
│  ┌─────────────────┐  ┌─────────────────────────────────┐  │
│  │ Python Analyzer │  │ Execution Sandbox               │  │
│  │                 │  │                                 │  │
│  │ - tree-sitter   │  │ ┌─────────────┐ ┌────────────┐ │  │
│  │ - import check  │  │ │ Pyodide     │ │ Native     │ │  │
│  │ - call analysis │  │ │ (warm WASM) │ │ (unshare/  │ │  │
│  │                 │  │ │             │ │ sandbox-   │ │  │
│  │                 │  │ │             │ │ exec)      │ │  │
│  │                 │  │ └─────────────┘ └────────────┘ │  │
│  └─────────────────┘  └─────────────────────────────────┘  │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ Daemon (optional, for warm Pyodide)                 │   │
│  │ - IPC socket for eval requests                      │   │
│  │ - Pre-loaded packages                               │   │
│  │ - Health monitoring                                 │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Flow

```
1. Bash hook receives: python -c '...'

2. Extract code from -c argument

3. Parse with tree-sitter-python

4. Classify:
   - Inspection mode: all imports known, all calls readonly
   - Execution mode: anything else

5. If inspection mode:
   - Return allow (native execution)

6. If execution mode:
   - Check if Pyodide can handle (imports available)
   - If yes: eval in Pyodide sandbox
   - If no: run in native network sandbox
   - Or: ask user
```

## Configuration

### Rego Policy Integration

```rego
# Allow inspection mode
rules["python_inspection"] := {
    "decision": "allow",
    "reason": "Safe Python inspection",
} if {
    input.binary_name == "python"
    input.parsed_flags.command  # -c flag present
    input.python_analysis.mode == "inspection"
    input.python_analysis.safe == true
}

# Sandbox execution mode
rules["python_execution"] := {
    "decision": "allow",  # or "ask" for extra caution
    "reason": "Python execution (sandboxed)",
} if {
    input.binary_name == "python"
    input.parsed_flags.command
    input.python_analysis.mode == "execution"
    input.sandbox_available == true
}
```

### User Configuration (Nickel)

```nickel
{
  python = {
    # Packages to trust for inspection mode
    trusted_imports = ["numpy", "pandas", "sqlalchemy"],

    # Sandbox preference
    sandbox = "pyodide",  # or "native" or "hybrid"

    # Pyodide pre-loaded packages
    pyodide_packages = ["numpy", "pandas", "requests"],
  }
}
```

## Implementation Phases

### Phase 1: Static Analysis (Inspection Mode)

1. Add tree-sitter-python dependency
2. Implement import extraction and validation
3. Implement readonly call detection
4. Integrate with command parser for `python -c`
5. Add Rego rules for inspection mode

**Outcome:** Safe inspection code auto-allowed

### Phase 2: Native Network Sandbox

1. Implement Linux sandbox (`unshare --net` or seccomp)
2. Implement macOS sandbox (`sandbox-exec` profile)
3. Add fallback for unsupported platforms (ask user)
4. Integrate with execution mode detection

**Outcome:** Execution code runs network-isolated

### Phase 3: Pyodide Integration (Optional)

1. Create daemon infrastructure (if not already present)
2. Integrate Pyodide runtime
3. Implement warm instance management
4. Add IPC protocol for eval requests
5. Package availability detection

**Outcome:** Full WASM sandboxing for compatible code

## Open Questions

1. **Subprocess handling:** Should subprocess-using code always ask, or try native sandbox?

2. **Secret reading:** Inspection mode allows reading files/env vars. Is this acceptable given Claude can already `cat` files?

3. **Performance budget:** What's acceptable latency for the static analysis path? (Currently Bash commands are <10ms)

4. **Package detection:** How to reliably find project dependencies? (pyproject.toml, requirements.txt, setup.py, conda environment.yml)

5. **Other interpreters:** Should this design extend to Node.js (`node -e`), Ruby (`ruby -e`), etc.?

## References

- [Pyodide documentation](https://pyodide.org/)
- [PEP 578 - Python Runtime Audit Hooks](https://peps.python.org/pep-0578/)
- [sandbox-exec man page](https://keith.github.io/xcode-man-pages/sandbox-exec.1.html)
- [seccomp-bpf](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html)
- [tree-sitter-python](https://github.com/tree-sitter/tree-sitter-python)
- [tree-sitter queries](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/index.html)
- [Semgrep](https://semgrep.dev/) - Pattern-based static analysis
- [Semgrep Python rules](https://semgrep.dev/p/python)
