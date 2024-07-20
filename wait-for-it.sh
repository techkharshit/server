#!/usr/bin/env bash

# wait-for-it.sh

set -e

host="$1"
shift
cmd="$@"

until nc -z "$host" 3306; do
  echo "Waiting for MySQL..."
  sleep 1
done

>&2 echo "MySQL is up - executing command"
exec $cmd
