#!/usr/bin/env bash
# Export the entries recorded in a deployed audit-log contract to JSON or CSV
# for offline review. Pages through `list_entries` (at most MAX_PAGE_SIZE
# entries per call) so the full history, or a ranged slice of it, is exported.
#
# Usage:
#   scripts/export-audit-log.sh <CONTRACT_ID> [--format json|csv] [--from N] [--to N] [--output FILE]
#
#   --format  json (default) or csv
#   --from    first zero-based entry index to export (default 0)
#   --to      last zero-based entry index to export, inclusive (default: last entry)
#   --output  write to FILE instead of stdout
#
# The network is read from $STELLAR_NETWORK (default "testnet") and the source
# identity from $STELLAR_SOURCE (default "default"). Requires stellar-cli and jq.
set -euo pipefail

PAGE_SIZE=100

usage() {
  echo "Usage: $0 <CONTRACT_ID> [--format json|csv] [--from N] [--to N] [--output FILE]" >&2
  exit 1
}

if [ "$#" -lt 1 ]; then
  usage
fi

CONTRACT_ID="$1"
shift

FORMAT="json"
FROM=0
TO=""
OUTPUT=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --format) FORMAT="${2:-}"; shift 2 ;;
    --from) FROM="${2:-}"; shift 2 ;;
    --to) TO="${2:-}"; shift 2 ;;
    --output) OUTPUT="${2:-}"; shift 2 ;;
    *)
      echo "error: unknown argument '$1'" >&2
      usage
      ;;
  esac
done

case "$FORMAT" in
  json | csv) ;;
  *)
    echo "error: --format must be 'json' or 'csv'" >&2
    usage
    ;;
esac

NETWORK="${STELLAR_NETWORK:-testnet}"
SOURCE_IDENTITY="${STELLAR_SOURCE:-default}"

invoke() {
  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$SOURCE_IDENTITY" \
    --network "$NETWORK" \
    --send=no \
    -- "$@"
}

COUNT="$(invoke entry_count)"
LAST=$((COUNT - 1))

if [ -z "$TO" ] || [ "$TO" -gt "$LAST" ]; then
  TO="$LAST"
fi

if [ "$LAST" -ge 0 ] && [ "$FROM" -gt "$TO" ]; then
  echo "error: --from ($FROM) is past the last exported index ($TO)" >&2
  exit 1
fi

# Each page is emitted as one JSON object per line, tagged with its zero-based
# index so the export keeps the on-chain ordering even after formatting.
ENTRIES=""
cursor="$FROM"
while [ "$cursor" -le "$TO" ]; do
  remaining=$((TO - cursor + 1))
  limit=$((remaining < PAGE_SIZE ? remaining : PAGE_SIZE))
  page="$(invoke list_entries --start "$cursor" --limit "$limit" \
    | jq -c --argjson start "$cursor" \
        'to_entries[] | {index: ($start + .key), ledger: .value.ledger, kind: .value.kind, source: .value.source, subject: .value.subject, detail: .value.detail}')"
  if [ -n "$page" ]; then
    ENTRIES+="${page}"$'\n'
  fi
  cursor=$((cursor + limit))
done

format_output() {
  if [ "$FORMAT" = "csv" ]; then
    echo "index,ledger,kind,source,subject,detail"
    printf '%s' "$ENTRIES" | jq -r -s '.[] | [.index, .ledger, .kind, .source, .subject, .detail] | @csv'
  else
    printf '%s' "$ENTRIES" | jq -s '.'
  fi
}

if [ -n "$OUTPUT" ]; then
  format_output > "$OUTPUT"
  echo "Exported entries ${FROM}-${TO} to ${OUTPUT}" >&2
else
  format_output
fi
