#!/usr/bin/env bash
set -e

DIR="$(dirname "$0")"

DESCRIPTION=$1
touch "$DIR/$(date +%s)_$DESCRIPTION.sql"
