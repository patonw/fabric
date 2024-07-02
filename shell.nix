{
  sources ? import ./nix/sources.nix {},
  pkgs ? import sources.nixpkgs {},
  ...
}:
let
  py = pkgs.python311;
  py-libs = with py.pkgs; [
    pip
  ];
in
{
  devShell = with pkgs; mkShellNoCC {
    buildInputs = [
      py-libs
      pipx
      poetry
      ffmpeg
    ];
  };
}
