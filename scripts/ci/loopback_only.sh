#!/bin/sh
# Run a proof as the invoking user in a fresh network namespace. The optional
# synthetic route makes a WebView detect "online" but has no external peer.
set -eu
mode=offline
if [ "${1:-}" = '--synthetic-online' ]; then
  mode=synthetic-online
  shift
fi
exec sudo unshare --net --fork --kill-child sh -ec '
  target_user=$1
  mode=$2
  shift 2
  ip link set lo up
  if [ "$mode" = synthetic-online ]; then
    ip link add cumulus-proof type dummy
    ip addr add 192.0.2.1/24 dev cumulus-proof
    ip link set cumulus-proof up
    ip route add default dev cumulus-proof
  fi
  exec runuser -u "$target_user" -- "$@"
' sh "$(id -un)" "$mode" "$@"
