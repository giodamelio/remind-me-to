{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    systems.url = "github:nix-systems/default";
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, systems, git-hooks, ... }:
    let
      forEachSystem = nixpkgs.lib.genAttrs (import systems);
    in
    {
      checks = forEachSystem (system: {
        package = self.packages.${system}.remind-me-to;
      });

      packages = forEachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          remind-me-to = pkgs.rustPlatform.buildRustPackage {
            pname = "remind-me-to";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeCheckInputs = [
              pkgs.cargo-nextest
              pkgs.git
            ];
            checkPhase = ''
              cargo nextest run --workspace
            '';
          };
        });

      devShells = forEachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          hooks = git-hooks.lib.${system}.run {
            src = ./.;
            package = pkgs.prek;
            hooks = {
              cargo-check.enable = true;
              rustfmt.enable = true;
              clippy.enable = true;
              cargo-nextest = {
                enable = true;
                name = "cargo-nextest";
                entry = "cargo nextest run";
                extraPackages = [ pkgs.cargo-nextest ];
                pass_filenames = false;
              };
            };
          };
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.remind-me-to ];
            shellHook = hooks.shellHook;
            buildInputs = hooks.enabledPackages;
            packages = [
              pkgs.cargo-insta
              pkgs.cargo-nextest
              pkgs.prek
            ];
          };
        });
    };
}
