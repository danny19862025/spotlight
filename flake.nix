{
  description = "Spotlight native Linux productivity launcher";

  # Immutable input revision; update deliberately when refreshing dependencies.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/09063bd1f2b36fd9fd793c36dfade7bd40a93dfc";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      packageFor = system:
        let pkgs = import nixpkgs { inherit system; };
        in pkgs.callPackage ./nix/package.nix { };
    in {
      packages = forAllSystems (system: {
        spotlight = packageFor system;
        default = self.packages.${system}.spotlight;
      });
      checks = forAllSystems (system: { inherit (self.packages.${system}) spotlight; });
      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.spotlight ];
            packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy ];
            GSK_RENDERER = "cairo";
            GDK_PIXBUF_MODULE_FILE = pkgs.gnome._gdkPixbufCacheBuilder_DO_NOT_USE {
              extraLoaders = [ pkgs.webp-pixbuf-loader ];
            };
          };
        });
    };
}
