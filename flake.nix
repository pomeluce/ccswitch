{
  description = "CCSwitch — Claude Code model configuration manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-parts,
      rust-overlay,
      home-manager,
      ...
    }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      perSystem =
        { system, ... }:
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
            version = "1.0.23";
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
            name = "ccswitch-dev";
            buildInputs = [
              rust
              pkgs.cargo
              pkgs.rust-analyzer
              pkgs.clippy
              pkgs.rustfmt
              pkgs.pkg-config
            ];
            shellHook = ''
              echo "🔄 CCSwitch dev shell"
              echo "  cargo build   — build"
              echo "  cargo test    — run tests"
              echo "  cargo run     — launch TUI"
              echo "  nix build .#  — build package"
            '';
          };
        };

      flake = {
        # NixOS system-level module
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
            };
            config = lib.mkIf cfg.enable {
              environment.systemPackages = [ self.packages.${pkgs.system}.default ];
            };
          };

        # Home Manager user-level module
        homeModules.default =
          {
            config,
            lib,
            pkgs,
            ...
          }:
          let
            cfg = config.programs.ccswitch;
          in
          {
            options.programs.ccswitch = {
              enable = lib.mkEnableOption "CCSwitch model configuration manager";
              defaults = lib.mkOption {
                type = lib.types.attrs;
                default = { };
                description = "Default provider configurations";
                example = {
                  version = 1;
                  providers = [
                    {
                      id = "deepseek";
                      name = "DeepSeek";
                      api_url = "https://api.deepseek.com/anthropic";
                      api_key = "env:DEEPSEEK_API_KEY";
                      profiles = [
                        {
                          id = "v4";
                          name = "V4";
                          opus = "deepseek-v4-pro[1m]";
                          sonnet = "deepseek-v4-pro[1m]";
                          haiku = "deepseek-v4-flash";
                          subagent = "deepseek-v4-flash";
                          default = true;
                        }
                      ];
                    }
                  ];
                };
              };
            };
            config = lib.mkIf cfg.enable {
              home.packages = [ self.packages.${pkgs.system}.default ];

              xdg.configFile."ccswitch/defaults.toml" =
                let
                  format = pkgs.formats.toml { };
                in
                {
                  source = format.generate "ccswitch-defaults.toml" cfg.defaults;
                };

              systemd.user.services.ccs-proxy = {
                Unit = {
                  Description = "CCSwitch Proxy Server";
                  After = [ "network.target" ];
                };
                Install = {
                  WantedBy = [ "default.target" ];
                };
                Service = {
                  ExecStart = "${self.packages.${pkgs.system}.default}/bin/ccs proxy serve";
                  Restart = "on-failure";
                  RestartSec = "5";
                };
              };
            };
          };
      };
    };
}
