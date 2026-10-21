# Entry point for the Nix User Repository.
#
# NUR imports this expression with a `pkgs` argument and expects the package
# set of this repository, so the same build definition serves the flake, NUR,
# and a plain `nix-build -A spotlight`.
{ pkgs ? import <nixpkgs> { } }:

{
  spotlight = pkgs.callPackage ./nix/package.nix { };
}
