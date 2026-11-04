#!/usr/bin/env bash
# Free disk space on a GitHub-hosted Ubuntu runner before a large Rust build.
#
# The workspace test and coverage builds write several gigabytes of test
# binaries and instrumented objects. On the Ubuntu 26 runner image they ran out
# of space ("No space left on device" from rustc and from the runner's own
# log writer). The GitHub-hosted Ubuntu runner ships SDKs this workflow never
# uses; removing them leaves room for the build. Directories that an image does
# not have are skipped. Do not run this on a self-hosted runner.
set -euo pipefail

echo "Disk before:"
df -h /

for dir in \
  /usr/share/dotnet \
  /usr/local/lib/android \
  /opt/ghc \
  /usr/local/.ghcup \
  /opt/hostedtoolcache/CodeQL \
  /usr/local/share/boost \
  /usr/share/swift; do
  if [ -d "$dir" ]; then
    sudo rm -rf "$dir"
  fi
done

echo "Disk after:"
df -h /
