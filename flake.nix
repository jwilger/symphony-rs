{
  description = "symphony-rs development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, rust-overlay, crane, flake-utils, advisory-db, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        projectName = "symphony-rs";
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        hasRustToolchainFile = builtins.pathExists ./rust-toolchain.toml;
        rustToolchain =
          if hasRustToolchainFile
          then pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
          else pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rustfmt" "clippy" "rust-src" "rust-analyzer" ];
          };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        hasCargoToml = builtins.pathExists ./Cargo.toml;
        src = if hasCargoToml then craneLib.cleanCargoSource ./. else ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          SQLX_OFFLINE = "true";

          buildInputs = [
            pkgs.pkg-config
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            pkgs.darwin.apple_sdk.frameworks.CoreServices
          ];

          nativeBuildInputs = [
            pkgs.pkg-config
          ];
        };

        cargoArtifacts = if hasCargoToml then craneLib.buildDepsOnly commonArgs else null;

        individualCrateArgs = commonArgs // pkgs.lib.optionalAttrs hasCargoToml {
          inherit cargoArtifacts;
          inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;
        };

        symphony_rs = if hasCargoToml then craneLib.buildPackage (individualCrateArgs // {
          pname = projectName;
        }) else null;
      in
      {
        checks = if hasCargoToml then {
          "${projectName}" = symphony_rs;

          "${projectName}-clippy" = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          "${projectName}-fmt" = craneLib.cargoFmt {
            inherit src;
          };

          "${projectName}-nextest" = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });

          "${projectName}-audit" = craneLib.cargoAudit {
            inherit src advisory-db;
          };

          "${projectName}-deny" = craneLib.cargoDeny {
            inherit src;
          };

          "${projectName}-doc" = craneLib.cargoDoc (commonArgs // {
            inherit cargoArtifacts;
          });
        } else {};

        packages = if hasCargoToml then {
          default = symphony_rs;
          "${projectName}" = symphony_rs;
        } else {};

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            git
            git-spice
            pre-commit
            nodejs_22
            glow
            just
            jq
            cargo-nextest
            cargo-watch
            cargo-audit
            cargo-deny
            cargo-expand
            cargo-machete
            cargo-leptos
            playwright-driver
            playwright-driver.browsers
            pkg-config
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            libiconv
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
            darwin.apple_sdk.frameworks.CoreServices
          ];

          shellHook = ''
            export PATH="''${PWD}/bin:''${PATH}"
            export PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}"
            export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
            git config --local rebase.updateRefs true 2>/dev/null || true
            git config --local --unset core.hooksPath 2>/dev/null || true
            pre-commit install 2>/dev/null || true

            echo "symphony-rs development environment loaded"
          '';
        };
      }
    );
}
