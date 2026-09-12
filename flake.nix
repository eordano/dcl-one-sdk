{
  description = "dcl-one-sdk — an npm-free Rust toolchain for Decentraland SDK7 scenes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  inputs.crane.url = "github:ipetkov/crane/v0.21.0";
  inputs.rust-overlay = { url = "github:oxalica/rust-overlay"; inputs.nixpkgs.follows = "nixpkgs"; };

  outputs = { self, nixpkgs, crane, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems
        (system: f (import nixpkgs { inherit system; }));
      lib = nixpkgs.lib;

      abgenLock =
        let
          text = builtins.readFile ./crates/dcl-one-sdk/abgen-release.lock;
          isEntry = l: builtins.match "[[:space:]]*[^#[:space:]][^=]*=.*" l != null;
          entry = l:
            let parts = lib.splitString "=" l;
            in lib.nameValuePair
              (lib.trim (builtins.head parts))
              (lib.trim (lib.concatStringsSep "=" (builtins.tail parts)));
        in
        builtins.listToAttrs (map entry (builtins.filter isEntry (lib.splitString "\n" text)));

      abgenTargets = {
        aarch64-darwin = "aarch64-apple-darwin";
        x86_64-darwin = "x86_64-apple-darwin";
        aarch64-linux = "aarch64-unknown-linux-gnu";
        x86_64-linux = "x86_64-unknown-linux-gnu";
      };
    in
    {
      packages = forAllSystems (pkgs:
        let
          system = pkgs.stdenv.hostPlatform.system;
          toolchain = (pkgs.extend (import rust-overlay)).rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

          abgen-dist =
            let
              target = abgenTargets.${system};
              archive = pkgs.fetchurl {
                url = builtins.replaceStrings
                  [ "{version}" "{target}" ] [ abgenLock.version target ] abgenLock.url;
                sha256 = abgenLock.${target};
              };
            in
            pkgs.runCommand "abgen-${abgenLock.version}-${target}" { } ''
              mkdir -p $out
              tar -xzf ${archive} -C $out --strip-components=1
              test -x $out/abgen
            '';

          sdkCraneArgs = {
            pname = "dcl-one-sdk";
            version = "0.24.1";
            src = ./.;
            strictDeps = true;
            cargoExtraArgs = "--locked -p dcl-one-sdk --bin dcl-one-sdk";
            doCheck = false;
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.openssl ]
              ++ nixpkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
            OPENSSL_NO_VENDOR = "1";
            ABGEN_EMBED_BIN = "${abgen-dist}/abgen";
          };

          dcl-one-sdk-deps = craneLib.buildDepsOnly sdkCraneArgs;

          dcl-one-sdk = craneLib.buildPackage (sdkCraneArgs // {
            cargoArtifacts = dcl-one-sdk-deps;
            meta.mainProgram = "dcl-one-sdk";
          });
        in
        {
          inherit abgen-dist dcl-one-sdk-deps dcl-one-sdk;
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
            ((pkgs.extend (import rust-overlay)).rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
            pkgs.pkg-config
          ];
          buildInputs = [ pkgs.openssl ];
          env.OPENSSL_NO_VENDOR = "1";
        };
      });
    };
}
