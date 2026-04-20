{
  pkgs,
  lib,
  config,
  inputs,
  ...
}: {
  languages.rust.enable = true;

  packages = [
    pkgs.cargo-insta
    pkgs.cargo-nextest
  ];

  git-hooks.hooks = {
    cargo-check.enable = true;
    rustfmt.enable = true;
    clippy.enable = true;
    cargo-test = {
      enable = true;
      name = "cargo-test";
      entry = "${lib.getExe config.languages.rust.toolchain.cargo} nextest run";
      pass_filenames = false;
    };
  };
}
