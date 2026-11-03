#!/bin/bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
"$project_dir/scripts/build-native.sh" "${1:-debug}"
exec "$project_dir/release/LocalProxy.app/Contents/MacOS/LocalProxy"
