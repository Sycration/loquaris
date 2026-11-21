#!/bin/sh
# Container entry point. Started as root (the default), it makes sure the
# data directory and everything in it belong to the server's user
# (LOQ_UID:LOQ_GID, 1000:1000 unless set) and runs the server as that user.
# Volumes from NAS systems and bind mounts often arrive owned by someone
# else, which would leave the server unable to create its database. Started
# as any other user (e.g. `docker run --user`), it runs the server directly.
set -eu

data="${LOQ_DATA_DIR:-/data}"

if [ "$(id -u)" != 0 ]; then
    exec loquaris-server "$@"
fi

uid="${LOQ_UID:-1000}"
gid="${LOQ_GID:-1000}"

mkdir -p "$data"
if [ -n "$(find "$data" \( ! -user "$uid" -o ! -group "$gid" \) -print -quit)" ]; then
    echo "Giving $data to user $uid:$gid so the server can write there."
    # Some storage refuses chown (e.g. ACL-managed shares); the server then
    # explains what it cannot write.
    chown -R "$uid:$gid" "$data" || echo "Could not change the owner of $data." >&2
fi
exec setpriv --reuid="$uid" --regid="$gid" --clear-groups loquaris-server "$@"
