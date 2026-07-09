{
  description = "dcl-one-sdk — an npm-free Rust toolchain for Decentraland SDK7 scenes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems
        (system: f (import nixpkgs { inherit system; }));
    in
    {
      packages = forAllSystems (pkgs: rec {
        dcl-one-sdk = pkgs.rustPlatform.buildRustPackage {
          pname = "dcl-one-sdk";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "dcl-one-sdk" "--bin" "dcl-one-sdk" ];
          doCheck = false;
          nativeBuildInputs = [ pkgs.pkg-config pkgs.protobuf ];
          buildInputs = [ pkgs.openssl ];
          env.OPENSSL_NO_VENDOR = "1";
          meta.mainProgram = "dcl-one-sdk";
        };
        default = dcl-one-sdk;
      });

      apps = forAllSystems (pkgs: rec {
        dcl-one-sdk = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.dcl-one-sdk}/bin/dcl-one-sdk";
        };
        default = dcl-one-sdk;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.pkg-config
            pkgs.protobuf
          ];
          buildInputs = [ pkgs.openssl ];
          env.OPENSSL_NO_VENDOR = "1";
        };
      });
    };
}
