{
  description = "skill-manager — declarative multi-harness AI skill deployment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAllSystems (pkgs: {
        default = import ./nix/devshell.nix { inherit pkgs; };
      });

      packages = forAllSystems (pkgs: rec {
        default = skill-manager;
        skill-manager = pkgs.rustPlatform.buildRustPackage {
          pname = "skill-manager";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          # The git-backed source tests shell out to the real client.
          nativeCheckInputs = [ pkgs.git ];
          preCheck = "export HOME=$(mktemp -d)";
          postInstall = ''
            wrapProgram $out/bin/skill-manager \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.git ]}
          '';
          meta = {
            description = "Installs, namespaces, and deploys AI agent skills from a declarative manifest";
            mainProgram = "skill-manager";
          };
        };
      });
    };
}
