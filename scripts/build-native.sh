#!/bin/bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
build_mode="${1:-debug}"
app_version="$(awk -F '"' '/^version[[:space:]]*=/ { print $2; exit }' "$project_dir/gpui-app/Cargo.toml")"
if [[ ! "$app_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][a-zA-Z0-9.-]+)?$ ]]; then
  printf 'Cannot determine application version from gpui-app/Cargo.toml\n' >&2
  exit 1
fi
case "$build_mode" in
  debug) cargo_profile=dev ;;
  release) cargo_profile=release ;;
  *) printf 'Usage: %s [debug|release]\n' "$0" >&2; exit 2 ;;
esac

# Build separately: each crate has its own lockfile and pinned toolchain.
(cd "$project_dir/gpui-app" && CARGO_TARGET_DIR="$project_dir/gpui-app/target" cargo build --locked --profile "$cargo_profile" --bin LocalProxy)
(cd "$project_dir/rust" && CARGO_TARGET_DIR="$project_dir/rust/target" cargo build --locked --profile "$cargo_profile" --bin local_proxy_wireguard_helper)

# The core resolves the privileged helper next to the running executable.
cp "$project_dir/rust/target/$build_mode/local_proxy_wireguard_helper" "$project_dir/gpui-app/target/$build_mode/"
app_bundle="$project_dir/release/LocalProxy.app"
mkdir -p "$app_bundle/Contents/MacOS" "$app_bundle/Contents/Resources"
cp "$project_dir/gpui-app/target/$build_mode/LocalProxy" "$app_bundle/Contents/MacOS/LocalProxy"
cp "$project_dir/rust/target/$build_mode/local_proxy_wireguard_helper" "$app_bundle/Contents/MacOS/local_proxy_wireguard_helper"
cp "$project_dir/assets/brand/localproxy.icns" "$app_bundle/Contents/Resources/localproxy.icns"
cat > "$app_bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>LocalProxy</string>
  <key>CFBundleIdentifier</key><string>com.localproxy.desktop</string>
  <key>CFBundleName</key><string>LocalProxy</string>
  <key>CFBundleDisplayName</key><string>LocalProxy</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>localproxy</string>
  <key>CFBundleShortVersionString</key><string>$app_version</string>
  <key>CFBundleVersion</key><string>$app_version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$app_bundle/Contents/MacOS/local_proxy_wireguard_helper"
codesign --force --deep --sign - "$app_bundle"
codesign --verify --deep --strict "$app_bundle"
printf 'Built %s\n' "$app_bundle"
