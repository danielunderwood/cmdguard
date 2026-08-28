{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixpkgs-stable.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-stable,
      flake-utils,
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

          devShells.default = pkgs.mkShell {
            buildInputs = [
              pkgs.cargo
              pkgs.nickel
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
    };
}
