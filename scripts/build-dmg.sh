#!/bin/bash
set -euo pipefail

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
app_version="$(awk -F '"' '/^version[[:space:]]*=/ { print $2; exit }' "$project_dir/gpui-app/Cargo.toml")"
if [[ ! "$app_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][a-zA-Z0-9.-]+)?$ ]]; then
  printf 'Cannot determine application version from gpui-app/Cargo.toml\n' >&2
  exit 1
fi
case "$(uname -m)" in
  arm64) app_arch=arm64 ;;
  x86_64) app_arch=x64 ;;
  *) printf 'Unsupported macOS architecture\n' >&2; exit 1 ;;
esac

# Keep packaging tools in the ignored build directory, independent of the
# removed Electron/Node project. Only install when missing or out of date.
tools_dir="$project_dir/gpui-app/target/dmg-tools"
tools_python="$tools_dir/bin/python"
if [ ! -x "$tools_python" ]; then
  python3 -m venv "$tools_dir"
fi
if ! "$tools_python" -c 'import importlib.metadata as m; assert all(m.version(n) == v for n, v in (("dmgbuild", "1.6.7"), ("ds_store", "1.3.3"), ("mac_alias", "2.2.3")))' 2>/dev/null; then
  "$tools_python" -m pip install --disable-pip-version-check -r "$project_dir/scripts/dmg-requirements.txt"
fi

"$project_dir/scripts/build-native.sh" release
app_bundle="$project_dir/release/LocalProxy.app"
final_dmg="$project_dir/release/LocalProxy-$app_version-$app_arch.dmg"
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/localproxy-dmg.XXXXXXXX")"
staged_dmg="$temp_dir/LocalProxy.dmg"
trap 'if [ -f "$staged_dmg" ]; then unlink "$staged_dmg"; fi; rmdir "$temp_dir" 2>/dev/null || true' EXIT

"$tools_dir/bin/dmgbuild" \
  -s "$project_dir/scripts/dmg-settings.py" \
  -D "app=$app_bundle" \
  -D "icon=$project_dir/assets/brand/localproxy.icns" \
  -D "background=$project_dir/assets/packaging/dmg-background.tiff" \
  LocalProxy "$staged_dmg"
hdiutil verify "$staged_dmg"
mv "$staged_dmg" "$final_dmg"
printf 'Built %s\n' "$final_dmg"
