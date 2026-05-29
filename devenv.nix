{ pkgs, lib, config, inputs, ... }:

{
  env = {
    GREET = "devenv";
    RUSTC_WRAPPER = "sccache";

    # GPUI's macOS backend links against real Apple frameworks (Metal, AppKit,
    # CoreVideo, ...) and its build scripts shell out to Apple's toolchain
    # (`xcrun metal`, `metallib`). devenv's default Nix `apple-sdk-14.4` is a
    # stub for cross-compilation pretend — it doesn't include the Metal
    # toolchain (Xcode 16+ ships it as an optional component) and doesn't
    # expose `libSystem` in a way the macOS linker can find. Point the SDK
    # and developer dir at the real Xcode install.
    DEVELOPER_DIR = "/Applications/Xcode.app/Contents/Developer";
    SDKROOT = "/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk";
  };

  packages = with pkgs; [
    git
    cmake
    sccache
  ];

  languages.rust = {
    enable = true;
    channel = "stable";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
      "rust-src"
    ];
  };

  # Strip Nix's `xcbuild-0.1.1-xcrun` stub from PATH so `xcrun` resolves to
  # the real `/usr/bin/xcrun` from Xcode, which knows about the cryptex-
  # mounted Metal toolchain. The Nix stub is a 2019 reimplementation that
  # predates Metal-as-optional-component.
  enterShell = ''
    export PATH=$(echo "$PATH" | tr ':' '\n' | grep -v xcbuild | paste -sd: -)
  '';

  # Project tasks. Run with `devenv tasks run <name>`, list with
  # `devenv tasks list`. All tasks inherit the env + PATH fixes above.
  tasks = {
    "openlogi:run" = {
      description = "Run the CLI: enumerate connected Logitech HID++ devices.";
      exec = "cargo run -p openlogi-cli -- list";
    };
    "openlogi:gui" = {
      description = "Run the GPUI desktop window.";
      exec = "cargo run -p openlogi-gui";
    };
    "openlogi:check" = {
      description = "Pre-commit check: fmt --check, clippy -D warnings, tests. Mirrors CI.";
      exec = ''
        set -e
        cargo fmt --all -- --check
        cargo clippy --workspace --all-targets -- -D warnings
        cargo test --workspace
      '';
    };
    "openlogi:assets" = {
      description = "Pull every device's bundle assets from assets.openlogi.org.";
      exec = "cargo run -p openlogi-cli --release -- assets sync";
    };
    "openlogi:bundle" = {
      description = "Build OpenLogi.app with assets baked into Resources/.";
      # Forces Xcode envs that the Nix apple-sdk hook otherwise overrides
      # (Metal toolchain + libSystem). cargo-bundle 0.10 resolves
      # `resources` globs against the process cwd, hence the cd into the
      # crate dir before invoking.
      exec = ''
        set -e
        export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
        export SDKROOT=$(/usr/bin/xcrun --sdk macosx --show-sdk-path)
        if ! command -v cargo-bundle >/dev/null; then
          export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/cc
          cargo install cargo-bundle --locked
        fi
        bash scripts/macos-icns.sh
        # Default: on-demand renders (lean .app, fetched at first launch).
        # OPENLOGI_BUNDLE_ASSETS=1 bakes every device's render in (offline).
        if [ "''${OPENLOGI_BUNDLE_ASSETS:-0}" = "1" ]; then
          cargo run -p openlogi-cli --release -- assets sync
        else
          rm -rf crates/openlogi-gui/assets
          mkdir -p crates/openlogi-gui/assets
        fi
        cd crates/openlogi-gui
        cargo bundle --release
        echo
        echo "Bundle ready: target/release/bundle/osx/OpenLogi.app"
      '';
    };
    "openlogi:dmg" = {
      description = "Build OpenLogi.app (icon + assets) and wrap it in a DMG.";
      # Full packaging: icon → assets → .app → optional codesign → .dmg.
      # Set OPENLOGI_SIGN_IDENTITY to a Developer ID to sign for release.
      exec = "bash scripts/package-macos.sh";
    };
  };
}
