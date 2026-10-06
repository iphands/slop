#!/bin/sh
# Fetch pinned ffmpeg-over-ip release binaries into ./bin/{server,client} (gitignored).
# server/ = ffmpeg-over-ip-server + its patched jellyfin-ffmpeg 7.1 (NVENC/NVDEC/tonemap_cuda) -> cosmo
# client/ = ffmpeg-over-ip-client (static)                                                    -> noir
set -eu
VER=v5.2.1
SERVER_SHA=d45b7c21895a65e0668cfc88b76a30d354f0cc3ba42e2641724dea15ae7132f8
CLIENT_SHA=6d6fd80749186b5a67a21607ab803cff864fcc87e0b6ff611e28a15d36fb7fe3
URL=https://github.com/steelbrain/ffmpeg-over-ip/releases/download/$VER

cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for kind in server client; do
    zip=linux-amd64-ffmpeg-over-ip-$kind.zip
    curl -fsSL -o "$tmp/$zip" "$URL/$zip"
    [ "$kind" = server ] && sha=$SERVER_SHA || sha=$CLIENT_SHA
    echo "$sha  $tmp/$zip" | sha256sum -c -
    rm -rf "bin/$kind"
    mkdir -p "bin/$kind"
    unzip -q "$tmp/$zip" -d "bin/$kind"
done
echo "$VER" > bin/VERSION
ls -la bin/server bin/client
