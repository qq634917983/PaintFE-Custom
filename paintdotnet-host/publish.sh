#!/usr/bin/env bash
set -euo pipefail

RID="${1:?usage: publish.sh <runtime-id> [output-dir]}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${2:-$ROOT/target/pdn-host/$RID}"

dotnet restore \
  "$ROOT/paintdotnet-host/src/PaintFE.PaintDotNetHost/PaintFE.PaintDotNetHost.csproj" \
  --locked-mode

dotnet publish \
  "$ROOT/paintdotnet-host/src/PaintFE.PaintDotNetHost/PaintFE.PaintDotNetHost.csproj" \
  -c Release -r "$RID" --self-contained true --no-restore -m:1 -o "$OUT"

HOST="$OUT/PaintFE.PaintDotNetHost"
if [[ "$RID" == win-* ]]; then
  HOST="$HOST.exe"
fi
sha256sum "$HOST" | awk '{print $1}' > "$HOST.sha256"
