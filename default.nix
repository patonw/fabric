{
  sources ? import ./nix/sources.nix {},
  pkgs ? import sources.nixpkgs { },
  ...
}:
let
  naersk = pkgs.callPackage sources.naersk { };
in
{
  app = naersk.buildPackage {
    name = "fabric";
    version = "0.1.0";
    src = ./.;
    buildInputs = [ ];
  };
}

