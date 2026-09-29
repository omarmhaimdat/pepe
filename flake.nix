{
  description = "pepe - HTTP load generator and performance testing tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      # Version tracks Cargo.toml, so release-plz bumps are picked up automatically
      manifest = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
    in
    {
      packages = forAllSystems (pkgs: {
        pepe = pkgs.rustPlatform.buildRustPackage {
          pname = manifest.name;
          version = manifest.version;
          src = pkgs.lib.cleanSource ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
            # Fetch the pinned curl-parser git dependency without a manual hash
            allowBuiltinFetchGit = true;
          };
          meta = {
            description = manifest.description;
            homepage = manifest.homepage;
            license = pkgs.lib.licenses.mit;
            mainProgram = "pepe";
          };
        };
        default = self.packages.${pkgs.system}.pepe;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.pepe ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
          ];
        };
      });
    };
}
