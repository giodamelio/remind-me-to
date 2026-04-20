{
  pkgs,
  lib,
  config,
  inputs,
  ...
}: {
  languages.rust.enable = true;

  git-hooks.hooks = {
    cargo-check.enable = true;
    rustfmt.enable = true;
    clippy.enable = true;
    cargo-test = {
      enable = true;
      name = "cargo-test";
      entry = "${lib.getExe config.languages.rust.toolchain.cargo} test";
      pass_filenames = false;
    };
  };
}
