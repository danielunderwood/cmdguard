{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixpkgs-stable.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";

    # Only used by `checks` to evaluate nix/home-manager.nix. Consumers never
    # force it, so it costs them nothing but keeps the module from rotting.
    # It follows nixpkgs, so both have to stay reasonably current: recent
    # home-manager modules read lib helpers that older nixpkgs lack.
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-stable,
      flake-utils,
      home-manager,
    }:
    let
      perSystem = flake-utils.lib.eachDefaultSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          pkgs-stable = nixpkgs-stable.legacyPackages.${system};
          manifest = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
        in
        {
          packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = manifest.name;
            version = manifest.version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.git ];

            # A resolver regression test intentionally verifies behavior from
            # inside a Git worktree; flake source archives omit .git metadata.
            preCheck = ''
              git init -q
            '';

            meta = {
              description = "Policy-driven permission control for AI coding agents";
              homepage = "https://github.com/danielunderwood/cmdguard";
              license = pkgs.lib.licenses.mit;
              mainProgram = "cmdguard";
            };
          };

          checks.home-manager-module =
            let
              homeDirectory = if pkgs.stdenv.hostPlatform.isDarwin then "/Users/check" else "/home/check";
              configDir = "${homeDirectory}/.config/cmdguard";
              # Same quoting the module applies, so the expectations below hold
              # whether or not lib decides this path needs quotes.
              dirArg = pkgs.lib.escapeShellArg configDir;
              # A stub keeps the check about module logic; the real package is
              # covered by `nix build .#default`.
              stub = pkgs.writeShellScriptBin "cmdguard" "exit 0";
              home = home-manager.lib.homeManagerConfiguration {
                inherit pkgs;
                modules = [
                  self.homeManagerModules.default
                  {
                    home = {
                      username = "check";
                      inherit homeDirectory;
                      stateVersion = "24.11";
                    };
                    programs.cmdguard = {
                      enable = true;
                      package = stub;
                      hookTargets = [
                        "claude"
                        "codex"
                      ];
                      policies.example = ''
                        package cmdguard

                        import rego.v1

                        denied_subcommands["git"] := {"push"}
                      '';
                      commands = ''
                        {
                          wrappers = {},
                          commands = {},
                        }
                      '';
                      policyTests = ''
                        tests: []
                      '';
                    };
                  }
                ];
              };
              declaredTargets = pkgs.lib.mapAttrsToList (_: file: file.target) home.config.home.file;
              entry = home.config.home.activation.cmdguard;
            in
            # Activation reads the linked policies, so it has to be ordered
            # after they exist.
            assert pkgs.lib.assertMsg (pkgs.lib.elem "linkGeneration" entry.after)
              "cmdguard activation must run after linkGeneration when policies are managed";
            pkgs.runCommand "cmdguard-home-manager-module-check"
              {
                activation = entry.data;
                # Absolute home.file targets have to land inside the generation
                # as home-relative paths, or managed policies never get linked.
                targets = pkgs.lib.concatStringsSep "\n" declaredTargets;
                # Building the generation exercises the module's assertions and
                # the file writers.
                generation = home.activationPackage;
              }
              ''
                printf '%s\n' "$activation" > activation.sh
                printf '%s\n' "$targets" > targets.txt

                expect() {
                  grep -qF -- "$1" activation.sh || {
                    echo "activation script is missing: $1" >&2
                    cat activation.sh >&2
                    exit 1
                  }
                }

                # Every policy-reading command must be pinned to configDir, and
                # the hooks must record that pin so GUI-launched agents without
                # XDG_CONFIG_HOME read the same policies.
                expect "base sync --policy-dir ${dirArg}"
                expect "validate --policy-dir ${dirArg}"
                expect "lint --policy-dir ${dirArg} --fail-on error"
                expect "test --policy-dir ${dirArg}"
                expect "hook install --target claude --policy-dir ${dirArg}"
                expect "hook install --target codex --policy-dir ${dirArg}"

                for target in .config/cmdguard/policies/example.rego \
                              .config/cmdguard/commands.ncl \
                              .config/cmdguard/policy_tests.yaml; do
                  grep -qxF -- "$target" targets.txt || {
                    echo "home.file does not manage $target" >&2
                    cat targets.txt >&2
                    exit 1
                  }
                  test -e "$generation/home-files/$target" || {
                    echo "$target is not present in the built generation" >&2
                    exit 1
                  }
                done

                touch $out
              '';

          checks.nix-formatting =
            pkgs.runCommand "cmdguard-nix-formatting"
              {
                nativeBuildInputs = [ pkgs.nixfmt ];
              }
              ''
                nixfmt --check ${./flake.nix} ${./nix}/*.nix
                touch $out
              '';

          formatter = pkgs.nixfmt;

          devShells.default = pkgs.mkShell {
            buildInputs = [
              pkgs.cargo
              pkgs.nickel
              pkgs.nixfmt
              pkgs.nls
              pkgs.rustc
              pkgs.rust-analyzer
              pkgs.clippy
              pkgs.rustfmt
              pkgs.regal
              pkgs-stable.open-policy-agent
            ];
          };
        }
      );
    in
    perSystem
    // {
      homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };
      # `homeModules` is the current naming convention; both names refer to the
      # same module so existing consumers keep working.
      homeModules.default = self.homeManagerModules.default;
    };
}
