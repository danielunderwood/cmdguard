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

    syncBasePolicies = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to refresh the embedded base policy bundle during Home Manager activation.";
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
    home.packages = [ cfg.package ];

    # The installer preserves unrelated hooks and user policies. Reinstalling
    # our entries also replaces stale immutable-store paths after an upgrade.
    home.activation.cmdguard = lib.hm.dag.entryAfter cfg.activationAfter ''
      ${lib.optionalString cfg.syncBasePolicies ''
        run ${cfg.package}/bin/cmdguard base sync
      ''}
      ${lib.concatMapStringsSep "\n" (target: ''
        run ${cfg.package}/bin/cmdguard hook uninstall --target ${target}
        ${lib.optionalString (lib.elem target cfg.hookTargets) ''
          run ${cfg.package}/bin/cmdguard hook install --target ${target}
        ''}
      '') supportedTargets}
    '';
  };
}
