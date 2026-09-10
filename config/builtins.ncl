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

    # sed's options are modelled in full: `allow_sed_in_pipe` only grants the
    # stdin -> stdout allowance when every option matched a definition, so an
    # undeclared option costs sed its allowance rather than passing unnoticed.
    # A flag definition holds one long form, so aliases (--silent) are separate
    # entries.
    sed = {
      flags = {
        # GNU sed accepts --in-place[=SUFFIX]. Leaving the long form undeclared
        # meant `sed --in-place=.bak f` set no in_place flag, so ask_sed_inplace
        # never fired and allow_sed_in_pipe treated it as stdout-only.
        in_place = { type = "with_optional_arg", short = ["-i"], long = "--in-place" },
        expression = repeat_flag "-e" "--expression",
        # A script file can contain `w` (write a file) and `e` (run a command).
        file = arg_flag "-f" "--file",
        quiet = bool_flag "-n" "--quiet",
        silent = bool_flag [] "--silent",
        regexp_extended = bool_flag ["-E", "-r"] "--regexp-extended",
        separate = bool_flag "-s" "--separate",
        null_data = bool_flag "-z" "--null-data",
        unbuffered = bool_flag "-u" "--unbuffered",
        line_length = arg_flag "-l" "--line-length",
        posix = bool_flag [] "--posix",
        debug = bool_flag [] "--debug",
        sandbox = bool_flag [] "--sandbox",
        follow_symlinks = bool_flag [] "--follow-symlinks",
        binary = bool_flag "-b" "--binary",
        help = bool_flag [] "--help",
        version = bool_flag [] "--version",
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
      # Modelling curl's options is what makes allowing curl possible at all: an
      # option cmdguard cannot see is an option that can redirect the request or
      # touch a local file, and network.rego refuses to allow a curl command
      # that carries an unmodelled flag.
      #
      # Options that policy only needs to *detect* are declared with `arg_flag`
      # rather than `repeat_flag`: a repeatable flag whose value is missing or
      # starts with a dash is dropped from `parsed_flags` entirely, while a
      # `with_arg` flag still records its presence. `repeat_flag` is reserved
      # for the flags whose every value has to be inspected.
      flags = {
        silent = bool_flag "-s" "--silent",
        show_error = bool_flag "-S" "--show-error",
        insecure = bool_flag "-k" "--insecure",
        request = arg_flag "-X" "--request",
        user = arg_flag "-u" "--user",
        disable = bool_flag "-q" "--disable",
        url = repeat_flag [] "--url",
        next = bool_flag "-:" "--next",

        # Redirect or remap the destination
        location = bool_flag "-L" "--location",
        location_trusted = bool_flag [] "--location-trusted",
        proto = arg_flag [] "--proto",
        proto_redir = arg_flag [] "--proto-redir",
        proto_default = arg_flag [] "--proto-default",
        config = arg_flag "-K" "--config",
        connect_to = arg_flag [] "--connect-to",
        resolve = arg_flag [] "--resolve",
        proxy = arg_flag "-x" "--proxy",
        preproxy = arg_flag [] "--preproxy",
        socks4 = arg_flag [] "--socks4",
        socks4a = arg_flag [] "--socks4a",
        socks5 = arg_flag [] "--socks5",
        socks5_hostname = arg_flag [] "--socks5-hostname",
        doh_url = arg_flag [] "--doh-url",
        unix_socket = arg_flag [] "--unix-socket",
        abstract_unix_socket = arg_flag [] "--abstract-unix-socket",
        interface = arg_flag [] "--interface",

        # Write a local file
        output = arg_flag "-o" "--output",
        remote_name = bool_flag "-O" "--remote-name",
        remote_header_name = bool_flag "-J" "--remote-header-name",
        remote_name_all = bool_flag [] "--remote-name-all",
        output_dir = arg_flag [] "--output-dir",
        create_dirs = bool_flag [] "--create-dirs",
        dump_header = arg_flag "-D" "--dump-header",
        cookie_jar = arg_flag "-c" "--cookie-jar",
        trace = arg_flag [] "--trace",
        trace_ascii = arg_flag [] "--trace-ascii",
        stderr = arg_flag [] "--stderr",
        etag_save = arg_flag [] "--etag-save",

        # Read a local file
        upload_file = arg_flag "-T" "--upload-file",
        cookie = arg_flag "-b" "--cookie",
        etag_compare = arg_flag [] "--etag-compare",
        netrc = bool_flag "-n" "--netrc",
        netrc_file = arg_flag [] "--netrc-file",
        cacert = arg_flag [] "--cacert",
        capath = arg_flag [] "--capath",
        cert = arg_flag "-E" "--cert",
        key = arg_flag [] "--key",

        # Read a local file when the value starts with `@`
        header = repeat_flag "-H" "--header",
        data = repeat_flag "-d" "--data",
        data_ascii = repeat_flag [] "--data-ascii",
        data_binary = repeat_flag [] "--data-binary",
        data_raw = repeat_flag [] "--data-raw",
        data_urlencode = repeat_flag [] "--data-urlencode",
        form = repeat_flag "-F" "--form",
        form_string = repeat_flag [] "--form-string",

        # Writes a local file itself: `-w '%output{file}'` (curl 7.83+)
        write_out = arg_flag "-w" "--write-out",
      },
      # curl accepts multiple bare URLs. Keep the established `url` field name,
      # but expose every value so policy can require that all targets are safe.
      positional = [strings_arg "url"],
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
