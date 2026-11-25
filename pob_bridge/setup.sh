#!/usr/bin/env bash
# Set up headless Path of Building (PoE1) for poecraft. Idempotent.
#   POB_HOME (default /home/user/pob): where PoB and the C modules are put (outside this repo).
# Pinned: PathOfBuildingCommunity/PathOfBuilding release 2.69.0
#         (commit 5606cc64fa9f15694e69001a52b90aa968d01a86, 2026-11-24, MIT-style licence).
set -euo pipefail
POB_HOME="${POB_HOME:-/home/user/pob}"
POB_COMMIT=5606cc64fa9f15694e69001a52b90aa968d01a86
HERE="$(cd "$(dirname "$0")" && pwd)"

# 1. LuaJIT, its headers (for luautf8) and lua-zlib (real Inflate/Deflate for timeless-jewel data)
if ! command -v luajit >/dev/null || [ ! -d /usr/include/luajit-2.1 ] || [ ! -f /usr/lib/x86_64-linux-gnu/lua/5.1/zlib.so ]; then
  apt-get install -y luajit libluajit-5.1-dev lua-zlib
fi

mkdir -p "$POB_HOME/deps"
# 2. luautf8 0.1.6 (the version PoB's own Dockerfile installs), built against LuaJIT
if [ ! -f "$POB_HOME/deps/luautf8/lua-utf8.so" ]; then
  git clone -q --depth 1 --branch 0.1.6 https://github.com/starwing/luautf8.git "$POB_HOME/deps/luautf8"
  gcc -O2 -shared -fPIC -I/usr/include/luajit-2.1 "$POB_HOME/deps/luautf8/lutf8lib.c" -o "$POB_HOME/deps/luautf8/lua-utf8.so"
fi

# 3. Path of Building at the pinned commit
if [ ! -d "$POB_HOME/PathOfBuilding/.git" ]; then
  git clone -q https://github.com/PathOfBuildingCommunity/PathOfBuilding.git "$POB_HOME/PathOfBuilding"
fi
cd "$POB_HOME/PathOfBuilding"
if [ "$(git rev-parse HEAD)" != "$POB_COMMIT" ]; then
  git fetch -q --depth 1 origin "$POB_COMMIT" && git checkout -q "$POB_COMMIT"
fi
# 4. Local fix: 2.69.0's CompareBuySimilar.lua uses '?.' and '..=' which stock LuaJIT rejects
if git apply --check "$HERE/pob-2.69.0-fix.patch" 2>/dev/null; then git apply "$HERE/pob-2.69.0-fix.patch"; fi

echo "OK. Try: cd $POB_HOME/PathOfBuilding/src && luajit $HERE/load_build.lua <build.xml> </dev/null"
