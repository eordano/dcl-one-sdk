{
  description = "dcl-one-sdk — an npm-free Rust toolchain for Decentraland SDK7 scenes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  inputs.crane.url = "github:ipetkov/crane/v0.21.0";

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems
        (system: f (import nixpkgs { inherit system; }));
      lib = nixpkgs.lib;

      # crates/dcl-one-sdk/abgen-release.lock is the one source of truth for the
      # abgen every build embeds. build.rs reads it and downloads; this reads the
      # same file and fetches the same archive, so a nix-built binary and a
      # `cargo build` one carry identical bytes. Parsing it here (rather than
      # taking abgen as a flake input and building it from source) is also what
      # keeps `nix build` from compiling the whole asset-bundle converter.
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

      # nix system -> abgen release target. The embed is a standalone
      # executable, so only os/arch matter.
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
          craneLib = crane.mkLib pkgs;

          # A release abgen is a relocatable bundle — launcher script, bin/, and
          # a bundled lib/ loader, with the Unity templates and shader bundles
          # compiled into the executable — so unpack the whole tree.
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

          # Args shared by the deps-only layer and the package build, kept
          # byte-identical so crane's cargoArtifacts actually hits. pname/version
          # are set explicitly because the root Cargo.toml is a virtual workspace
          # manifest (no [package]), so crane cannot derive them from it.
          sdkCraneArgs = {
            pname = "dcl-one-sdk";
            version = "0.20.0";
            src = ./.;
            strictDeps = true;
            cargoExtraArgs = "--locked -p dcl-one-sdk --bin dcl-one-sdk";
            doCheck = false;
            nativeBuildInputs = [ pkgs.pkg-config pkgs.protobuf ];
            buildInputs = [ pkgs.openssl ]
              ++ nixpkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
            OPENSSL_NO_VENDOR = "1";
            ABGEN_EMBED_BIN = "${abgen-dist}/abgen";
          };

          # third-party dep graph compiled once into its own derivation, reused
          # by the package build.
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
