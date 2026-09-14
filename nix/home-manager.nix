{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.cmdguard;
  supportedTargets = [
    "claude"
    "codex"
  ];

  cmdguard = "${cfg.package}/bin/cmdguard";
  policyDirArg = lib.escapeShellArg cfg.configDir;

  # A path expression means "use this file as-is"; a string is policy source.
  # Store paths spelled as strings are indistinguishable from source here, so
  # the option docs tell users to pass paths unquoted.
  fileEntry = value: if builtins.isPath value then { source = value; } else { text = value; };

  policyFiles = lib.mapAttrs' (
    name: value: lib.nameValuePair "${cfg.configDir}/policies/${name}.rego" (fileEntry value)
  ) cfg.policies;

  managedFiles =
    policyFiles
    // lib.optionalAttrs (cfg.commands != null) {
      "${cfg.configDir}/commands.ncl" = fileEntry cfg.commands;
    }
    // lib.optionalAttrs (cfg.policyTests != null) {
      "${cfg.configDir}/policy_tests.yaml" = fileEntry cfg.policyTests;
    };

  # Managed policies are symlinked by linkGeneration, so anything that reads
  # them has to run after it. `base sync` only seeds policies/custom.rego when
  # policies/ does not exist yet, so running it after linkGeneration also keeps
  # it from dropping a starter template next to declared policies.
  activationDeps = lib.unique (
    cfg.activationAfter ++ lib.optional (managedFiles != { }) "linkGeneration"
  );
in
{
  options.programs.cmdguard = {
    enable = lib.mkEnableOption "cmdguard policy enforcement for coding-agent shell hooks";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "cmdguard.packages.\${pkgs.system}.default";
      description = "cmdguard package to install and register with enabled hooks.";
    };

    configDir = lib.mkOption {
      type = lib.types.str;
      default = "${config.home.homeDirectory}/.config/cmdguard";
      defaultText = lib.literalExpression ''"''${config.home.homeDirectory}/.config/cmdguard"'';
      description = ''
        Directory holding cmdguard's policies. Managed policies are written
        here, and registered hooks are pinned to it with `--policy-dir`.

        This deliberately ignores {option}`xdg.configHome`: an agent launched
        from a GUI hands its hooks no `XDG_CONFIG_HOME`, so cmdguard's own
        resolution would fall back to `~/.config/cmdguard` there while picking
        the XDG path in a terminal. Pinning one directory keeps both on the same
        policies. Set this to `"''${config.xdg.configHome}/cmdguard"` if you
        want the XDG location regardless.
      '';
    };

    syncBasePolicies = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to refresh the embedded base policy bundle during Home Manager activation.";
    };

    policies = lib.mkOption {
      type = lib.types.attrsOf (lib.types.either lib.types.lines lib.types.path);
      default = { };
      example = lib.literalExpression ''
        {
          # Written to <configDir>/policies/work.rego
          work = '''
            package cmdguard

            import rego.v1

            denied_subcommands["git"] := {"push"}
          ''';
          # Unquoted path: the file is used as-is.
          shared = ./cmdguard/shared.rego;
        }
      '';
      description = ''
        Rego policies written to `<configDir>/policies/<name>.rego`, evaluated
        alongside the shipped base policies. Files you drop in that directory by
        hand are left alone, so managed and unmanaged policies can coexist.
      '';
    };

    commands = lib.mkOption {
      type = lib.types.nullOr (lib.types.either lib.types.lines lib.types.path);
      default = null;
      example = lib.literalExpression ''
        '''
          {
            wrappers = {},
            commands = {
              my_tool = {
                flags = {
                  verbose = { short = ["-v"], long = "--verbose", type = "boolean" },
                },
              },
            },
          }
        '''
      '';
      description = ''
        Nickel wrapper and command definitions written to
        `<configDir>/commands.ncl`. Validated during activation, so a
        malformed config fails the switch instead of surfacing later inside an
        agent.
      '';
    };

    policyTests = lib.mkOption {
      type = lib.types.nullOr (lib.types.either lib.types.lines lib.types.path);
      default = null;
      example = lib.literalExpression ''
        '''
          tests:
            - name: "deny git push"
              command: "git push origin main"
              expect: deny
        '''
      '';
      description = ''
        Policy tests written to `<configDir>/policy_tests.yaml` and run during
        activation. A failing test fails the switch, which is the point: it
        keeps a broken policy set from reaching your agents.
      '';
    };

    lintPolicies = lib.mkOption {
      type = lib.types.bool;
      default = cfg.policies != { } || cfg.commands != null;
      defaultText = lib.literalExpression "true when policies or commands are managed";
      description = ''
        Run `cmdguard lint` during activation and fail the switch on errors.
      '';
    };

    hookTargets = lib.mkOption {
      type = lib.types.listOf (
        lib.types.enum [
          "claude"
          "codex"
        ]
      );
      default = [ "claude" ];
      description = "Coding-agent hook protocols registered during Home Manager activation.";
    };

    activationAfter = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "writeBoundary" ];
      description = ''
        Home Manager activation entries that must finish before cmdguard edits
        agent hook files. Add the activation names of modules that merge those
        same files.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        # home.file resolves absolute targets by stripping $HOME, so a config
        # directory outside it would silently never be linked.
        assertion = managedFiles == { } || lib.hasPrefix "${config.home.homeDirectory}/" cfg.configDir;
        message = "programs.cmdguard.configDir must be inside ${config.home.homeDirectory} to manage policies declaratively, got ${cfg.configDir}.";
      }
    ];

    home.packages = [ cfg.package ];

    home.file = managedFiles;

    # The installer preserves unrelated hooks and user policies. Reinstalling
    # our entries also replaces stale immutable-store paths after an upgrade.
    home.activation.cmdguard = lib.hm.dag.entryAfter activationDeps ''
      ${lib.optionalString cfg.syncBasePolicies ''
        run ${cmdguard} base sync --policy-dir ${policyDirArg}
      ''}
      ${lib.optionalString (cfg.commands != null) ''
        run ${cmdguard} validate --policy-dir ${policyDirArg}
      ''}
      ${lib.optionalString cfg.lintPolicies ''
        run ${cmdguard} lint --policy-dir ${policyDirArg} --fail-on error
      ''}
      ${lib.optionalString (cfg.policyTests != null) ''
        run ${cmdguard} test --policy-dir ${policyDirArg}
      ''}
      ${lib.concatMapStringsSep "\n" (target: ''
        run ${cmdguard} hook uninstall --target ${target}
        ${lib.optionalString (lib.elem target cfg.hookTargets) ''
          run ${cmdguard} hook install --target ${target} --policy-dir ${policyDirArg}
        ''}
      '') supportedTargets}
    '';
  };
}
