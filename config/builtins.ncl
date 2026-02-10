# Built-in command definitions for claude-permissions
#
# This file is loaded automatically before user's commands.ncl.
# Users can override any definition by redefining it in their config.

# =============================================================================
# Helper Functions
# =============================================================================

# Create a boolean flag
let bool_flag = fun short_form long_form => {
  type = "boolean",
  short = if std.is_array short_form then short_form else [short_form],
  long = long_form,
}
in

# Create a flag that takes an argument
let arg_flag = fun short_form long_form => {
  type = "with_arg",
  short = if std.is_array short_form then short_form else [short_form],
  long = long_form,
}
in

# Create a repeatable flag (can appear multiple times)
let repeat_flag = fun short_form long_form => {
  type = "repeatable",
  short = if std.is_array short_form then short_form else [short_form],
  long = long_form,
}
in

# Create a path positional argument
let path_arg = fun arg_name => {
  name = arg_name,
  type = "path",
}
in

# Create a variadic path argument (multiple values)
let paths_arg = fun arg_name => {
  name = arg_name,
  type = "path",
  variadic = true,
}
in

# Create a string positional argument
let string_arg = fun arg_name => {
  name = arg_name,
  type = "string",
}
in

# Create a variadic string argument
let strings_arg = fun arg_name => {
  name = arg_name,
  type = "string",
  variadic = true,
}
in

# Create a "last" positional argument (like cp destination)
let last_arg = fun arg_name arg_type => {
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

    tail = head & {
      flags = head.flags & {
        follow = bool_flag "-f" "--follow",
      },
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
        log = stub,
        diff = stub,
        branch = stub,
        checkout = stub,
        commit = stub,
        pull = stub,
        fetch = stub,
        merge = stub,
        rebase = stub,
        stash = stub,
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
  },

  # Default parsing options
  defaults = {
    combine_short_flags = true,
    double_dash_ends_flags = true,
  },
}
