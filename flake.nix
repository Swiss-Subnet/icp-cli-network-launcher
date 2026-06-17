{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {inherit system overlays;};

        # Matches rust-toolchain.toml.
        rustToolchain = pkgs.rust-bin.stable."1.96.0".default.override {
          extensions = ["rustfmt" "clippy"];
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = [rustToolchain];
        };
      }
    );
}
