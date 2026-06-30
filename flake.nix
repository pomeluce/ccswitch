{
  description = "CCSwitch — Claude Code model configuration manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rust = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rust;
          rustc = rust;
        };
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = "ccswitch";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = [ pkgs.installShellFiles ];
          postInstall = ''
            installShellCompletion --zsh --name _ccs \
              <($out/bin/ccs completions zsh)
            installShellCompletion --bash ccs \
              <($out/bin/ccs completions bash)
            installShellCompletion --fish ccs \
              <($out/bin/ccs completions fish)
            installManPage <($out/bin/ccs man)
          '';
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustPlatform
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
          ];
        };
      }
    )
    // {
      nixosModules.default =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.ccswitch;
        in
        {
          options.services.ccswitch = {
            enable = lib.mkEnableOption "CCSwitch model configuration manager";
            defaults = lib.mkOption {
              type = lib.types.attrs;
              default = { };
              description = "System default provider configurations";
            };
          };
          config = lib.mkIf cfg.enable {
            environment.etc."ccswitch/defaults.toml" = {
              text = builtins.toTOML cfg.defaults;
              mode = "0444";
            };
            systemd.user.services.ccs-proxy = {
              description = "CCSwitch Proxy Server";
              after = [ "network.target" ];
              wantedBy = [ "default.target" ];
              serviceConfig = {
                ExecStart = "${self.packages.${pkgs.system}.default}/bin/ccs proxy serve";
                Restart = "on-failure";
                RestartSec = "5";
              };
            };
          };
        };
    };
}
