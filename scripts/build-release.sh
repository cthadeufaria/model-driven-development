#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(cargo metadata --format-version 1 --no-deps --manifest-path "$repo_root/Cargo.toml" | node -e 'let data=""; process.stdin.on("data", c => data += c); process.stdin.on("end", () => { const meta = JSON.parse(data); const pkg = meta.packages.find(p => p.name === "mdd-cli"); console.log(pkg.version); });')"
target_triple="$(rustc -vV | awk '/host:/ { print $2 }')"
release_dir="$repo_root/dist/mdd-$version-$target_triple"
plantuml_version="1.2026.3"
plantuml_sha256="53af6760d96bb2737e5e4386e832b46339fc29dec74f412d7c12db7c30db8ec4"
plantuml_url="https://github.com/plantuml/plantuml/releases/download/v${plantuml_version}/plantuml.jar"
plantuml_dir="$repo_root/third_party/plantuml"
plantuml_jar="$plantuml_dir/plantuml.jar"
plantuml_notice="$plantuml_dir/plantuml.txt"

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    echo "error: shasum or sha256sum is required to verify plantuml.jar" >&2
    exit 1
  fi
}

if [[ ! -f "$plantuml_jar" ]]; then
  if [[ "${MDD_DOWNLOAD_PLANTUML:-0}" == "1" ]]; then
    mkdir -p "$plantuml_dir"
    curl -L --fail --show-error -o "$plantuml_jar" "$plantuml_url"
  else
    echo "error: missing $plantuml_jar" >&2
    echo "Set MDD_DOWNLOAD_PLANTUML=1 to download PlantUML $plantuml_version from $plantuml_url." >&2
    exit 1
  fi
fi

actual_plantuml_sha256="$(sha256_file "$plantuml_jar")"
if [[ "$actual_plantuml_sha256" != "$plantuml_sha256" ]]; then
  echo "error: $plantuml_jar checksum mismatch" >&2
  echo "expected: $plantuml_sha256" >&2
  echo "actual:   $actual_plantuml_sha256" >&2
  exit 1
fi

if [[ ! -f "$plantuml_notice" ]]; then
  echo "error: missing $plantuml_notice" >&2
  exit 1
fi

cd "$repo_root"
cargo build --release -p mdd-cli

rm -rf "$release_dir"
mkdir -p "$release_dir/bin" "$release_dir/share/mdd/licenses"
cp "$repo_root/target/release/mdd" "$release_dir/bin/mdd"
cp "$plantuml_jar" "$release_dir/share/mdd/plantuml.jar"
cp "$plantuml_notice" "$release_dir/share/mdd/licenses/plantuml.txt"

tarball="$repo_root/dist/mdd-$version-$target_triple.tar.gz"
tar -C "$repo_root/dist" -czf "$tarball" "mdd-$version-$target_triple"

echo "$tarball"
