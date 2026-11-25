#!/usr/bin/env bash
set -euo pipefail

args=()
for arg in "$@"; do
  if [[ "$arg" == @* ]]; then
    rsp="${arg#@}"
    if [[ -f "$rsp" ]]; then
      sed -i 's/"-lstdc++"/"-l:libstdc++.a"/g; s/-lstdc++/-l:libstdc++.a/g' "$rsp"
    fi
    args+=("$arg")
  elif [[ "$arg" == "-lstdc++" ]]; then
    args+=("-l:libstdc++.a")
  else
    args+=("$arg")
  fi
done

exec x86_64-w64-mingw32-gcc "${args[@]}"
