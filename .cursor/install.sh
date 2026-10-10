#!/usr/bin/env bash
# Cloud Agent bootstrap. Runs once per environment build (then snapshotted), so
# it must stay idempotent: every step is a no-op or a cheap refresh on re-run.
set -euo pipefail
cd "$(dirname "$0")/.."

# Same Linux set as the clippy/test jobs in .github/workflows/ci.yml.
pkgs=(
  libudev-dev
  gcc g++ clang libfontconfig-dev libwayland-dev
  libxkbcommon-x11-dev libx11-xcb-dev
  libssl-dev libzstd-dev pkg-config
)
missing=()
for p in "${pkgs[@]}"; do
  dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'ok installed' || missing+=("$p")
done
if ((${#missing[@]})); then
  echo "==> apt: installing ${missing[*]}"
  sudo apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${missing[@]}"
fi

if ! command -v rustup >/dev/null; then
  echo "==> installing rustup"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
  for bin in "$HOME"/.cargo/bin/*; do sudo ln -sf "$bin" /usr/local/bin/; done
fi

# rust-toolchain.toml pins `stable` + rustfmt/clippy; also make it the default
# outside the repo so nothing falls back to an older image toolchain.
echo "==> rustup: stable"
rustup toolchain install stable --profile minimal --component rustfmt,clippy --no-self-update
rustup default stable
rustc --version

echo "==> cargo fetch"
cargo fetch --locked

# Warm target/ for the commands agents run most. A compile error on the default
# branch must not fail the environment build, so these only warn.
echo "==> warming build cache"
cargo clippy --workspace --all-targets --locked || echo "warning: cargo clippy failed during warm-up" >&2
cargo test --workspace --no-run --locked || echo "warning: cargo test --no-run failed during warm-up" >&2
