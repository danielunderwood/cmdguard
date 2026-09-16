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

          # Shared fixture for every Home Manager check below, so a new
          # configuration costs one `mkHome` call rather than a copy of the
          # whole scaffold.
          hmHome = if pkgs.stdenv.hostPlatform.isDarwin then "/Users/check" else "/home/check";
          hmConfigDir = "${hmHome}/.config/cmdguard";
          # Same quoting the module applies, so the expectations below hold
          # whether or not lib decides this path needs quotes.
          hmDirArg = pkgs.lib.escapeShellArg hmConfigDir;
          # A stub keeps eval-level checks about module logic; checks that need
          # real behavior pass the built package instead.
          hmStub = pkgs.writeShellScriptBin "cmdguard" "exit 0";
          examplePolicy = ''
            package cmdguard

            import rego.v1

            denied_subcommands["git"] := {"push"}
          '';
          mkHome =
            cmdguardCfg:
            home-manager.lib.homeManagerConfiguration {
              inherit pkgs;
              modules = [
                self.homeManagerModules.default
                {
                  home = {
                    username = "check";
                    homeDirectory = hmHome;
                    stateVersion = "24.11";
                  };
                  programs.cmdguard = {
                    enable = true;
                    package = hmStub;
                  }
                  // cmdguardCfg;
                }
              ];
            };
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
              dirArg = hmDirArg;
              home = mkHome {
                hookTargets = [
                  "claude"
                  "codex"
                ];
                policies.example = examplePolicy;
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

          # Activation must touch only the targets it manages. Asserting the
          # absence of `hook uninstall` is the point: the grep-for-presence
          # check above passes just as happily when activation also tears down
          # every other target on the way through.
          checks.home-manager-hook-targets =
            let
              home = mkHome { hookTargets = [ "claude" ]; };
            in
            pkgs.runCommand "cmdguard-home-manager-hook-targets"
              { activation = home.config.home.activation.cmdguard.data; }
              ''
                printf '%s\n' "$activation" > activation.sh

                reject() {
                  if grep -qF -- "$1" activation.sh; then
                    echo "activation must never contain: $1" >&2
                    cat activation.sh >&2
                    exit 1
                  fi
                }

                # Activation cannot tell a hook this module wrote from one the
                # user wrote, so it must not remove hooks at all.
                reject "hook uninstall"
                # codex is not in hookTargets, so nothing may address it.
                reject "--target codex"

                grep -qF -- "hook install --target claude" activation.sh || {
                  echo "the claude hook is not registered" >&2
                  cat activation.sh >&2
                  exit 1
                }

                touch $out
              '';

          # Runs the activation script for real, against the built binary and a
          # writable home. The grep-based check above only proves which command
          # lines were emitted; this one proves what they do to an agent's
          # settings file, which is where the destructive bugs lived.
          checks.home-manager-activation =
            let
              home = mkHome {
                package = self.packages.${system}.default;
                hookTargets = [ "claude" ];
                policies.example = examplePolicy;
              };
            in
            pkgs.runCommand "cmdguard-home-manager-activation"
              {
                nativeBuildInputs = [ pkgs.jq ];
                activation = home.config.home.activation.cmdguard.data;
                generation = home.activationPackage;
              }
              ''
                export HOME="$TMPDIR/check-home"
                mkdir -p "$HOME"

                # Home Manager links managed files before this entry runs
                # (activationDeps includes linkGeneration); reproduce that.
                cp -RL "$generation/home-files/." "$HOME/"
                chmod -R u+w "$HOME"

                # The generation bakes in an absolute home directory. Retarget
                # it at the sandbox's writable home so the check needs no
                # privileged paths and behaves the same on Linux and Darwin.
                printf '%s\n' "$activation" | sed "s|${hmHome}|$HOME|g" > activation.sh

                # A hook the user installed by hand, with decoration cmdguard
                # never emits. Nothing in this activation addresses codex, so it
                # must come out byte for byte identical.
                mkdir -p "$HOME/.codex"
                cat > "$HOME/.codex/hooks.json" <<'JSON'
                {"hooks":{"PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"RUST_LOG=debug /usr/local/bin/cmdguard hook run --target codex 2>>/tmp/cg.log"}]}]}}
                JSON
                cp "$HOME/.codex/hooks.json" expected-codex.json

                # An unrelated use of the binary in the agent cmdguard *does*
                # manage. Refreshing our own entry must not collect it.
                mkdir -p "$HOME/.claude"
                cat > "$HOME/.claude/settings.json" <<'JSON'
                {"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/usr/local/bin/cmdguard eval \"$CMD\" >> /tmp/audit.log"}]}]}}
                JSON

                # Stand in for Home Manager's own `run` helper.
                run() { "$@"; }
                . ./activation.sh

                fail() { echo "$1" >&2; shift; "$@" >&2 || true; exit 1; }

                ls "$HOME/.config/cmdguard/base/"*.rego > /dev/null 2>&1 \
                  || fail "base policies were not synced" ls -R "$HOME/.config/cmdguard"

                test -f "$HOME/.config/cmdguard/policies/example.rego" \
                  || fail "the managed policy is not in the config dir" ls -R "$HOME/.config/cmdguard"

                command=$(jq -r '[.hooks.PreToolUse[].hooks[].command] | map(select(test("hook run"))) | .[0]' \
                  "$HOME/.claude/settings.json")
                case "$command" in
                  *"--policy-dir"*) ;;
                  *) fail "the registered hook carries no --policy-dir pin: $command" \
                       cat "$HOME/.claude/settings.json" ;;
                esac
                case "$command" in
                  "$HOME/.config/cmdguard"*|*"$HOME/.config/cmdguard"*) ;;
                  *) fail "the hook is pinned somewhere other than the managed config dir: $command" \
                       cat "$HOME/.claude/settings.json" ;;
                esac

                jq -e '[.hooks.PreToolUse[].hooks[].command] | any(test("audit\\.log"))' \
                  "$HOME/.claude/settings.json" > /dev/null \
                  || fail "an unrelated cmdguard invocation was deleted" cat "$HOME/.claude/settings.json"

                cmp -s expected-codex.json "$HOME/.codex/hooks.json" \
                  || fail "a hand-installed codex hook was modified" \
                       diff expected-codex.json "$HOME/.codex/hooks.json"

                # A switch that changes nothing must rewrite nothing.
                cp "$HOME/.claude/settings.json" before-resettle.json
                . ./activation.sh
                cmp -s before-resettle.json "$HOME/.claude/settings.json" \
                  || fail "a second activation rewrote settings.json" \
                       diff before-resettle.json "$HOME/.claude/settings.json"

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
