{
  description = "dcl-one-sdk — an npm-free Rust toolchain for Decentraland SDK7 scenes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  # The asset-bundle generator. Without it this builds a toolchain whose
  # `start` has no bundler to hand: build.rs falls back to an EMPTY embed, and
  # asset bundles then depend on the user separately supplying abgen through
  # ABGEN_BIN, an npm package or PATH. Upstream is public, so there is no reason
  # for the standalone build to be weaker than the one inside the monorepo.
  inputs.abgen.url = "github:decentraland/abgen";

  outputs = { self, nixpkgs, abgen }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems
        (system: f (import nixpkgs { inherit system; }));
    in
    {
      packages = forAllSystems (pkgs: rec {
        # The layout both ABGEN_EMBED_BIN (compile-time embed) and ABGEN_BIN
        # expect: the binary with template/ and shader/ as siblings. build.rs
        # panics if any of the three is missing rather than embedding a
        # half-archive, so keep them together.
        abgen-dist =
          let system = pkgs.stdenv.hostPlatform.system;
          in pkgs.runCommand "abgen-dist" { } ''
            mkdir -p $out
            cp ${abgen.packages.${system}.default}/bin/abgen $out/abgen
            cp -r ${abgen}/template $out/template
            cp -r ${abgen}/crate/shader $out/shader
          '';

        dcl-one-sdk = pkgs.rustPlatform.buildRustPackage {
          pname = "dcl-one-sdk";
          version = "0.16.5";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "dcl-one-sdk" "--bin" "dcl-one-sdk" ];
          doCheck = false;
          nativeBuildInputs = [ pkgs.pkg-config pkgs.protobuf ];
          buildInputs = [ pkgs.openssl ]
            ++ nixpkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          env.OPENSSL_NO_VENDOR = "1";
          env.ABGEN_EMBED_BIN = "${abgen-dist}/abgen";
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
