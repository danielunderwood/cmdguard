# Built-in command definitions for cmdguard
#
# This file is loaded automatically before user's commands.ncl.
# Users can override any definition by redefining it in their config.

# =============================================================================
# Helper Functions
# =============================================================================

# Create a boolean flag
let bool_flag = fun short_form long_form =>
  {
    type = "boolean",
    short = if std.is_array short_form then short_form else [short_form],
    long = long_form,
  }
in

# Create a flag that takes an argument
let arg_flag = fun short_form long_form =>
  {
    type = "with_arg",
    short = if std.is_array short_form then short_form else [short_form],
    long = long_form,
  }
in

# Create a repeatable flag (can appear multiple times)
let repeat_flag = fun short_form long_form =>
  {
    type = "repeatable",
    short = if std.is_array short_form then short_form else [short_form],
    long = long_form,
  }
in

# Create a path positional argument
let path_arg = fun arg_name =>
  {
    name = arg_name,
    type = "path",
  }
in

# Create a variadic path argument (multiple values)
let paths_arg = fun arg_name =>
  {
    name = arg_name,
    type = "path",
    variadic = true,
  }
in

# Create a string positional argument
let string_arg = fun arg_name =>
  {
    name = arg_name,
    type = "string",
  }
in

# Create a variadic string argument
let strings_arg = fun arg_name =>
  {
    name = arg_name,
    type = "string",
    variadic = true,
  }
in

# Create a "last" positional argument (like cp destination)
let last_arg = fun arg_name arg_type =>
  {
    name = arg_name,
    type = arg_type,
    last = true,
  }
in

# Create an empty subcommand stub
let stub = {} in

# Create multiple empty subcommand stubs
let stubs = fun names =>
  std.array.fold_left
    (fun acc name => std.record.insert name {} acc)
    {}
    names
in

# =============================================================================
# Common Flags (reusable across commands)
# =============================================================================
let common = {
  verbose = bool_flag "-v" "--verbose",
  quiet = bool_flag "-q" "--quiet",
  help = bool_flag "-h" "--help",
  version = bool_flag "-V" "--version",
  force = bool_flag "-f" "--force",
  recursive = bool_flag ["-r", "-R"] "--recursive",
}
in

# =============================================================================
# Command Definitions
# =============================================================================
{
  commands = {
    # -------------------------------------------------------------------------
    # File Operations
    # -------------------------------------------------------------------------
    rm = {
      flags = {
        recursive = bool_flag ["-r", "-R"] "--recursive",
        force = bool_flag "-f" "--force",
        no_preserve_root = bool_flag [] "--no-preserve-root",
      },
      positional = [paths_arg "targets"],
    },

    chmod = {
      flags = {
        recursive = bool_flag "-R" "--recursive",
      },
      positional = [
        { name = "mode", type = "string", position = 0 },
        paths_arg "targets",
      ],
    },

    chown = {
      flags = {
        recursive = bool_flag "-R" "--recursive",
      },
      positional = [
        { name = "owner", type = "string", position = 0 },
        paths_arg "targets",
      ],
    },

    cp = {
      flags = {
        recursive = bool_flag ["-r", "-R"] "--recursive",
        force = bool_flag "-f" "--force",
        no_clobber = bool_flag "-n" "--no-clobber",
      },
      positional = [
        paths_arg "sources",
        last_arg "destination" "path",
      ],
    },

    mv = {
      flags = {
        force = bool_flag "-f" "--force",
        no_clobber = bool_flag "-n" "--no-clobber",
      },
      positional = [
        paths_arg "sources",
        last_arg "destination" "path",
      ],
    },

    mkdir = {
      flags = {
        parents = bool_flag "-p" "--parents",
      },
      positional = [paths_arg "directories"],
    },

    touch = {
      positional = [paths_arg "files"],
    },

    cat = {
      flags = {
        number = bool_flag "-n" "--number",
      },
      positional = [{ name = "files", type = "path", variadic = true, optional = true }],
    },

    ls = {
      flags = {
        long = bool_flag "-l" null,
        all = bool_flag "-a" "--all",
        human_readable = bool_flag "-h" "--human-readable",
      },
      positional = [{ name = "paths", type = "path", variadic = true, optional = true }],
    },

    find = {
      flags = {
        type = arg_flag [] "-type",
        name = arg_flag [] "-name",
        path = arg_flag [] "-path",
        maxdepth = arg_flag [] "-maxdepth",
        exec = bool_flag [] "-exec",
        delete = bool_flag [] "-delete",
      },
      positional = [{ name = "paths", type = "path", variadic = true, optional = true }],
    },

    sed = {
      flags = {
        in_place = { type = "with_optional_arg", short = ["-i"], long = null },
        expression = arg_flag "-e" null,
        quiet = bool_flag "-n" null,
      },
      positional = [
        string_arg "script",
        { name = "files", type = "path", variadic = true, optional = true },
      ],
    },

    grep = {
      flags = {
        recursive = bool_flag ["-r", "-R"] "--recursive",
        ignore_case = bool_flag "-i" "--ignore-case",
        files_with_matches = bool_flag "-l" "--files-with-matches",
        invert_match = bool_flag "-v" "--invert-match",
        line_number = bool_flag "-n" "--line-number",
        count = bool_flag "-c" "--count",
        word_regexp = bool_flag "-w" "--word-regexp",
        context = arg_flag "-C" "--context",
      },
      positional = [
        string_arg "pattern",
        { name = "files", type = "path", variadic = true, optional = true },
      ],
    },

    rg = {
      flags = {
        ignore_case = bool_flag "-i" "--ignore-case",
        files_with_matches = bool_flag "-l" "--files-with-matches",
        invert_match = bool_flag "-v" "--invert-match",
        line_number = bool_flag "-n" "--line-number",
        count = bool_flag "-c" "--count",
        word_regexp = bool_flag "-w" "--word-regexp",
        type_filter = arg_flag "-t" "--type",
        glob = arg_flag "-g" "--glob",
        context = arg_flag "-C" "--context",
      },
      positional = [
        string_arg "pattern",
        { name = "paths", type = "path", variadic = true, optional = true },
      ],
    },

    # head/tail support legacy -NUM syntax (e.g., tail -30)
    # Disable short flag combining so -30 isn't expanded to -3 -0
    head = {
      parsing = { combine_short_flags = false },
      flags = {
        # lines supports legacy -NUM syntax (e.g., -30 means -n 30)
        lines = arg_flag "-n" "--lines" & { claim_pattern = "^-(\\d+)$" },
        bytes = arg_flag "-c" "--bytes",
        quiet = common.quiet,
        verbose = common.verbose,
      },
      positional = [{ name = "files", type = "path", variadic = true, optional = true }],
    },

    tail =
      head
      & {
        flags =
          head.flags
          & {
            follow = bool_flag "-f" "--follow",
          },
      },

    # -------------------------------------------------------------------------
    # Network / File Transfer
    # -------------------------------------------------------------------------
    wget = {
      flags = {
        output_document = arg_flag "-O" "--output-document",
        quiet = bool_flag "-q" "--quiet",
        recursive = bool_flag "-r" "--recursive",
        level = arg_flag "-l" "--level",
        no_clobber = bool_flag "-nc" "--no-clobber",
      },
      positional = [strings_arg "urls"],
    },

    rsync = {
      flags = {
        archive = bool_flag "-a" "--archive",
        verbose = bool_flag "-v" "--verbose",
        delete = bool_flag [] "--delete",
        dry_run = bool_flag "-n" "--dry-run",
        compress = bool_flag "-z" "--compress",
        exclude = repeat_flag [] "--exclude",
      },
      positional = [
        paths_arg "sources",
        last_arg "destination" "path",
      ],
    },

    tee = {
      flags = {
        append = bool_flag "-a" "--append",
      },
      positional = [paths_arg "files"],
    },

    # -------------------------------------------------------------------------
    # Wrappers
    # -------------------------------------------------------------------------
    sudo = {
      is_wrapper = true,
      flags = {
        user = arg_flag "-u" "--user",
        group = arg_flag "-g" "--group",
        stdin = bool_flag "-S" "--stdin",
        login = bool_flag "-i" "--login",
        preserve_env = bool_flag "-E" "--preserve-env",
      },
    },

    xargs = {
      is_wrapper = true,
      flags = {
        null_terminated = bool_flag "-0" null,
        max_args = arg_flag "-n" null,
        max_procs = arg_flag "-P" null,
      },
    },

    timeout = {
      is_wrapper = true,
    },

    # -------------------------------------------------------------------------
    # Git
    # -------------------------------------------------------------------------
    git = {
      flags = {
        directory = arg_flag "-C" null,
      },
      subcommands = {
        status = {
          flags = {
            short = bool_flag "-s" "--short",
            branch = bool_flag "-b" "--branch",
          },
        },
        push = {
          flags = {
            force = bool_flag "-f" "--force",
            force_with_lease = bool_flag [] "--force-with-lease",
            delete = bool_flag "-d" "--delete",
            set_upstream = bool_flag "-u" "--set-upstream",
          },
        },
        reset = {
          flags = {
            hard = bool_flag [] "--hard",
            soft = bool_flag [] "--soft",
            mixed = bool_flag [] "--mixed",
          },
        },
        clean = {
          flags = {
            force = bool_flag "-f" "--force",
            directories = bool_flag "-d" null,
            ignored = bool_flag "-x" null,
          },
        },
        rev-parse = {
          positional = [string_arg "revision"],
        },
        show = stub,
        ls-tree = stub,
        ls-files = stub,
        check-ignore = stub,
        worktree = {
          positional = [string_arg "subcommand"],
        },
        log = stub,
        diff = stub,
        grep = stub,
        branch = stub,
        checkout = stub,
        commit = stub,
        pull = stub,
        fetch = stub,
        merge = stub,
        rebase = stub,
        restore = stub,
        stash = stub,
        switch = stub,
        version = stub,
        add = stub,
        rm = stub,
        mv = stub,
      },
    },

    # -------------------------------------------------------------------------
    # Cargo (Rust)
    # -------------------------------------------------------------------------
    cargo = {
      subcommands = {
        build = {
          flags = {
            release = bool_flag "-r" "--release",
            all_features = bool_flag [] "--all-features",
          },
        },
        run = {
          flags = {
            release = bool_flag "-r" "--release",
          },
        },
        test = {
          flags = {
            release = bool_flag "-r" "--release",
            no_run = bool_flag [] "--no-run",
          },
        },
        check = stub,
        clean = stub,
        clippy = stub,
        fmt = {
          flags = {
            check = bool_flag [] "--check",
          },
        },
        doc = stub,
        bench = stub,
        publish = stub,
        update = stub,
      },
    },

    # -------------------------------------------------------------------------
    # npm (JavaScript/Node)
    # -------------------------------------------------------------------------
    npm = {
      subcommands = {
        install = {
          flags = {
            save_dev = bool_flag "-D" "--save-dev",
            global = bool_flag "-g" "--global",
          },
          positional = [{ name = "packages", type = "string", variadic = true, optional = true }],
        },
        run = {
          positional = [{ name = "script", type = "string", position = 0 }],
        },
        test = stub,
        ci = stub,
        start = stub,
        build = stub,
      },
    },

    yarn = {
      subcommands = {
        add = {
          flags = {
            dev = bool_flag "-D" "--dev",
          },
          positional = [{ name = "packages", type = "string", variadic = true, optional = true }],
        },
        remove = {
          positional = [strings_arg "packages"],
        },
        run = {
          positional = [string_arg "script"],
        },
        test = stub,
        build = stub,
        install = stub,
        dev = stub,
        start = stub,
      },
    },

    pnpm = {
      subcommands = {
        add = {
          flags = {
            save_dev = bool_flag "-D" "--save-dev",
            global = bool_flag "-g" "--global",
          },
          positional = [{ name = "packages", type = "string", variadic = true, optional = true }],
        },
        remove = {
          positional = [strings_arg "packages"],
        },
        run = {
          positional = [string_arg "script"],
        },
        test = stub,
        build = stub,
        install = stub,
        dev = stub,
        start = stub,
      },
    },

    # -------------------------------------------------------------------------
    # Python
    # -------------------------------------------------------------------------
    python = {
      flags = {
        module = arg_flag "-m" null,
        code = arg_flag "-c" null,
      },
      positional = [path_arg "file"],
    },

    python3 = {
      flags = {
        module = arg_flag "-m" null,
        code = arg_flag "-c" null,
      },
      positional = [path_arg "file"],
    },

    # -------------------------------------------------------------------------
    # pip (Python)
    # -------------------------------------------------------------------------
    pip = {
      subcommands = {
        install = {
          flags = {
            upgrade = bool_flag "-U" "--upgrade",
            requirements = arg_flag "-r" "--requirement",
            editable = arg_flag "-e" "--editable",
          },
          positional = [{ name = "packages", type = "string", variadic = true, optional = true }],
        },
        uninstall = {
          flags = {
            yes = bool_flag "-y" "--yes",
          },
          positional = [strings_arg "packages"],
        },
        freeze = stub,
        list = stub,
        show = stub,
      },
    },

    pip3 = {
      subcommands = {
        install = {
          flags = {
            upgrade = bool_flag "-U" "--upgrade",
            requirements = arg_flag "-r" "--requirement",
            editable = arg_flag "-e" "--editable",
          },
          positional = [{ name = "packages", type = "string", variadic = true, optional = true }],
        },
        uninstall = {
          flags = {
            yes = bool_flag "-y" "--yes",
          },
          positional = [strings_arg "packages"],
        },
        freeze = stub,
        list = stub,
        show = stub,
      },
    },

    # -------------------------------------------------------------------------
    # make
    # -------------------------------------------------------------------------
    make = {
      flags = {
        jobs = arg_flag "-j" "--jobs",
        file = arg_flag "-f" "--file",
        directory = arg_flag "-C" "--directory",
      },
      positional = [{ name = "targets", type = "string", variadic = true, optional = true }],
    },

    # -------------------------------------------------------------------------
    # curl
    # -------------------------------------------------------------------------
    curl = {
      flags = {
        silent = bool_flag "-s" "--silent",
        output = arg_flag "-o" "--output",
        location = bool_flag "-L" "--location",
        header = repeat_flag "-H" "--header",
        data = arg_flag "-d" "--data",
        request = arg_flag "-X" "--request",
        user = arg_flag "-u" "--user",
        insecure = bool_flag "-k" "--insecure",
      },
      positional = [last_arg "url" "string"],
    },

    # -------------------------------------------------------------------------
    # Docker
    # -------------------------------------------------------------------------
    docker = {
      subcommands = {
        run = {
          flags = {
            rm = bool_flag [] "--rm",
            interactive = bool_flag "-i" "--interactive",
            tty = bool_flag "-t" "--tty",
            volume = repeat_flag "-v" "--volume",
            env = repeat_flag "-e" "--env",
            publish = repeat_flag "-p" "--publish",
          },
          positional = [string_arg "image"],
        },
        build = {
          flags = {
            tag = arg_flag "-t" "--tag",
            file = arg_flag "-f" "--file",
          },
          positional = [last_arg "context" "path"],
        },
        exec = {
          flags = {
            interactive = bool_flag "-i" "--interactive",
            tty = bool_flag "-t" "--tty",
          },
          positional = [{ name = "container", type = "string", position = 0 }],
        },
        ps = {
          flags = {
            all = bool_flag "-a" "--all",
          },
        },
        images = stub,
        pull = stub,
        push = stub,
        stop = stub,
        rm = stub,
        rmi = stub,
      },
    },

    # -------------------------------------------------------------------------
    # GitHub CLI
    # -------------------------------------------------------------------------
    gh = {
      subcommands = {
        pr = {
          subcommands =
            stubs ["checks", "close", "comment", "create", "diff", "edit", "list", "merge", "ready", "review", "view"],
        },
        issue = {
          subcommands =
            stubs ["close", "comment", "create", "delete", "edit", "list", "pin", "reopen", "status", "transfer", "unpin", "view"],
        },
        run = {
          subcommands = stubs ["cancel", "delete", "download", "list", "rerun", "view", "watch"],
        },
        repo = {
          subcommands = stubs ["archive", "clone", "create", "delete", "edit", "fork", "list", "rename", "view"],
        },
        api = {
          positional = [strings_arg "args"],
        },
        search = {
          subcommands = stubs ["code", "commits", "issues", "prs", "repos"],
        },
        auth = {
          subcommands = stubs ["login", "logout", "refresh", "status", "token"],
        },
      },
    },
  },

  # Default parsing options
  defaults = {
    combine_short_flags = true,
    double_dash_ends_flags = true,
  },
}
