#!/usr/bin/env bash
# Builds what mesa_clc, the host tool of the Intel and AMD Vulkan drivers'
# builds, links and Ubuntu 24.04 has too old or not at all: SPIRV-Tools 2024.1
# or newer, and SPIRV-LLVM-Translator for LLVM 21.1. Both are static, with
# their pkg-config files under PREFIX/lib/pkgconfig.
#
# Usage: spirv.sh PREFIX. Needs git, cmake, ninja, a C++ compiler and LLVM
# 21's CMake files under /usr/lib/llvm-21.
set -euo pipefail

# Each source is pinned by tag and by the commit the tag named when pinned.
TOOLS_TAG=v2026.1
TOOLS_COMMIT=fbe4f3ad913c44fe8700545f8ffe35d1382b7093
# SPIRV-Tools' DEPS at that tag.
TOOLS_HEADERS=04f10f650d514df88b76d25e83db360142c7b174
TRANSLATOR_TAG=v21.1.5
TRANSLATOR_COMMIT=a76fd0702de564476fb31e8a15f1415f05ad8951
# SPIRV-LLVM-Translator's spirv-headers-tag.conf at that tag.
TRANSLATOR_HEADERS=9268f3057354a2cb65991ba5f38b16d81e803692
LLVM_DIR=/usr/lib/llvm-21/lib/cmake/llvm

prefix="${1:?usage: spirv.sh PREFIX}"
mkdir -p "$prefix"
prefix="$(cd "$prefix" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# fetch DIRECTORY URL REVISION COMMIT: REVISION alone, without history,
# which must be COMMIT.
fetch() {
  git init -q "$1"
  git -C "$1" fetch -q --depth 1 "$2" "$3"
  git -C "$1" checkout -q FETCH_HEAD
  local commit
  commit="$(git -C "$1" rev-parse HEAD)"
  [[ "$commit" == "$4" ]] || { echo "$2 $3 is $commit, not $4" >&2; exit 1; }
}

github=https://github.com/KhronosGroup
fetch "$work/tools" "$github/SPIRV-Tools" "refs/tags/$TOOLS_TAG" "$TOOLS_COMMIT"
fetch "$work/tools-headers" "$github/SPIRV-Headers" "$TOOLS_HEADERS" "$TOOLS_HEADERS"
fetch "$work/translator" "$github/SPIRV-LLVM-Translator" "refs/tags/$TRANSLATOR_TAG" "$TRANSLATOR_COMMIT"
fetch "$work/translator-headers" "$github/SPIRV-Headers" "$TRANSLATOR_HEADERS" "$TRANSLATOR_HEADERS"

# Position-independent: mesa_clc is a position-independent executable.
common=(
  -G Ninja
  -DCMAKE_BUILD_TYPE=Release
  -DCMAKE_POSITION_INDEPENDENT_CODE=ON
  -DCMAKE_INSTALL_PREFIX="$prefix"
  -DCMAKE_INSTALL_LIBDIR=lib
)
cmake -S "$work/tools" -B "$work/tools/build" "${common[@]}" \
  -DSPIRV-Headers_SOURCE_DIR="$work/tools-headers" \
  -DSPIRV_SKIP_TESTS=ON \
  -DSPIRV_SKIP_EXECUTABLES=ON \
  -DSPIRV_WERROR=OFF
cmake --build "$work/tools/build"
cmake --install "$work/tools/build"

cmake -S "$work/translator" -B "$work/translator/build" "${common[@]}" \
  -DLLVM_DIR="$LLVM_DIR" \
  -DLLVM_EXTERNAL_SPIRV_HEADERS_SOURCE_DIR="$work/translator-headers" \
  -DLLVM_SPIRV_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_TESTS=OFF
cmake --build "$work/translator/build"
cmake --install "$work/translator/build"

PKG_CONFIG_PATH="$prefix/lib/pkgconfig" pkg-config --modversion SPIRV-Tools LLVMSPIRVLib
