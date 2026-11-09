#!/bin/sh
# Initialize only loopback in a fresh network namespace, then run the proof
# as the invoking user. No host network interface or route is inherited.
set -eu
exec sudo unshare --net --fork --kill-child sh -ec '
  target_user=$1
  shift
  ip link set lo up
  exec runuser -u "$target_user" -- "$@"
' sh "$(id -un)" "$@"
