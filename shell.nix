{
  sources ? import ./nix/sources.nix {},
  rust-overlay ? import sources.rust-overlay,
  pkgs ? import sources.nixpkgs {
    overlays = [ rust-overlay ];
  },
  ...
}:
let
  py = pkgs.python311;
  py-libs = with py.pkgs; [
    pip
  ];
  toolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [ "rust-src" ];
  };
  naersk = pkgs.callPackage sources.naersk {
    rustc = toolchain;
    cargo = toolchain;
  };
  libraries = with pkgs; [
    openssl
  ];
in
{
  devShell = with pkgs; mkShellNoCC {
    nativeBuildInputs = [
      cargo
      toolchain
      pkg-config
    ] ++ libraries;
    buildInputs = [
      py-libs
      pipx
      poetry
      ffmpeg
    ];

    shellHook = ''
      export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath libraries}:$LD_LIBRARY_PATH
    '';
  };
}
