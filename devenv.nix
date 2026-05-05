{
  pkgs,
  lib,
  config,
  inputs,
  ...
}:

{
  env.GREET = "rm-apps";
  packages = [
    pkgs.git
    pkgs.rustup
    pkgs.openssl
    pkgs.python314
    pkgs.lld
    pkgs.rust-analyzer
  ];

  languages.rust.enable = true;

  scripts.rustupdate.exec = ''
    rustup toolchain install nightly
    rustup default stable
  '';

  enterShell = ''
    rustupdate
    git --version
  '';
  
}
