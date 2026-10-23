{
  description = "Sandboxed local-machine MCP server";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "local-mcp";
            version = "0.1.0";
            src = self;

            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = with pkgs; [
              makeWrapper
            ];

            postInstall = ''
              wrapProgram $out/bin/local-mcp \
                --prefix PATH : ${
                  pkgs.lib.makeBinPath (
                    [
                      pkgs.bash
                      pkgs.git
                    ]
                    ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.bubblewrap ]
                  )
                }
            '';

            meta = {
              description = "Session-owned MCP adapter for Codex App Server";
              homepage = "https://github.com/openai/codex";
              license = pkgs.lib.licenses.asl20;
              mainProgram = "local-mcp";
              platforms = supportedSystems;
            };
          };
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/local-mcp";
          meta.description = "Run the local-mcp stdio server";
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              git
              rustc
              rustfmt
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.bubblewrap ];
          };
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-tree);
    };
}
