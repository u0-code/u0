#!/bin/sh
# Back up one ghost while she runs.
#
#   deploy/backup.sh /var/lib/ghost/weiwei /var/backups/ghost [DAYS_KEPT]
#
# Each run makes /var/backups/ghost/weiwei/<time>/ holding journal.db,
# work.db and files.tar, and removes runs older than DAYS_KEPT (14 by
# default). Copy the backup directory off this machine as well: a backup on
# the disk it backs up survives everything but losing the disk.
#
# What is in it, and why:
#   journal.db  her life. It is append-only and forever, and nothing can
#               rebuild it. This is the one file that must never be lost.
#   work.db     the state of what she has taken on: plans, notes, transcripts.
#   files.tar   ghost.toml, assets/ and media/: who she is as last written,
#               how she looks to others, and the images she was sent. The
#               words her brain is given are shared by every character and
#               are not in her directory: back up the deployment's prompts
#               directory with the rest of /etc/ghost.
# What is not: derived/, which is rebuilt from the journal when she starts.
#
# Needs sqlite3 and tar. To restore: stop her, put journal.db and work.db
# back in her directory, unpack files.tar there, and run `ghostd verify DIR`
# before starting her again.
set -eu

usage="usage: backup.sh GHOST_DIR BACKUP_DIR [DAYS_KEPT]"
ghost=${1:?$usage}
into=${2:?$usage}
keep=${3:-14}

[ -f "$ghost/journal.db" ] || {
    echo "no journal.db in $ghost: is that her directory?" >&2
    exit 1
}

name=$(basename "$ghost")
dest="$into/$name/$(date -u +%Y%m%dT%H%M%SZ)"
# Her journal is private: so is every copy of it.
umask 077
mkdir -p "$dest"

# A plain copy of a database that is being written can catch it halfway
# through a transaction. SQLite's own backup takes a consistent snapshot,
# and does not stop her to do it.
for db in journal.db work.db; do
    if [ -f "$ghost/$db" ]; then
        sqlite3 "$ghost/$db" ".backup '$dest/$db'"
    fi
done

# A copy that cannot be read back is not a backup.
check=$(sqlite3 "$dest/journal.db" "PRAGMA integrity_check")
if [ "$check" != "ok" ]; then
    echo "the copy of $ghost/journal.db does not check out: $check" >&2
    exit 1
fi

# These files are written whole and not changed afterwards, so an archive
# taken while she runs is consistent.
set --
for part in ghost.toml assets media; do
    if [ -e "$ghost/$part" ]; then
        set -- "$@" "$part"
    fi
done
tar -C "$ghost" -cf "$dest/files.tar" "$@"

# Only directories this script made are removed: one per run, named by its
# time, under this ghost's own directory of backups.
find "$into/$name" -mindepth 1 -maxdepth 1 -type d -name '????????T??????Z' \
    -mtime +"$keep" -exec rm -rf {} +

echo "backed up $ghost to $dest"
