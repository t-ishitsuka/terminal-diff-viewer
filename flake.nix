{
  description = "ターミナルでディレクトリツリーと git 差分を閲覧する TUI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        tdv = pkgs.rustPlatform.buildRustPackage {
          pname = "tdv";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # 統合テストは一時ディレクトリに実リポジトリを作るため git が要る
          nativeCheckInputs = [ pkgs.git ];

          meta = {
            description = "ディレクトリツリーと git 差分をターミナルで閲覧する TUI";
            mainProgram = "tdv";
            license = pkgs.lib.licenses.mit;
            platforms = pkgs.lib.platforms.unix;
          };
        };
        default = tdv;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer git ];
        };
      });
    };
}
