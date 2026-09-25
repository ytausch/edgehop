{
  description = "Switch Logitech Easy-Switch devices when the cursor hits a screen edge";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      # edgehop builds only for macOS and Windows, and releases target Apple
      # Silicon only.
      systems = [ "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.edgehop;
        edgehop = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          inherit (cargoToml.package) version;
          # Only the files the build reads, so that changes to docs or CI keep
          # the store path, and with it the Input Monitoring grant.
          src = nixpkgs.lib.fileset.toSource {
            root = ./.;
            fileset = nixpkgs.lib.fileset.unions [
              ./.cargo
              ./Cargo.lock
              ./Cargo.toml
              ./config.example.toml
              ./src
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;

          meta = {
            inherit (cargoToml.package) description;
            homepage = cargoToml.package.repository;
            license = nixpkgs.lib.licenses.mit;
            mainProgram = "edgehop";
            platforms = systems;
          };
        };
      });
    };
}
