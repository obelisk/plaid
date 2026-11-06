#!/bin/bash
#
# Shared helpers for Plaid integration test harnesses.
#
# Harnesses start `request_handler` in the background and then immediately
# drive a webhook. The rule under test makes a named request back to that
# handler, so the handler MUST already be accepting connections before the
# webhook is fired. Relying on a fixed `sleep` for this is racy: on a cold or
# loaded CI runner the handler can take longer to bind than the webhook
# round-trip, the rule's named request fails with ECONNREFUSED, and the rule
# panics on `.unwrap()`. These helpers replace the guesswork with a real
# readiness check.

# Wait until a TCP port on localhost accepts connections.
#
# Usage: wait_for_port <port> [timeout_seconds]
wait_for_port() {
  local port="${1:-8998}"
  local timeout="${2:-30}"
  local deadline=$((SECONDS + timeout))

  while (( SECONDS < deadline )); do
    if (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; then
      exec 3<&- 3>&-
      return 0
    fi
    sleep 0.05
  done

  echo "Timed out after ${timeout}s waiting for 127.0.0.1:${port} to accept connections" >&2
  return 1
}

# Wait until the request handler started by the harness is listening.
#
# Aborts the harness with a clear message if the handler never binds or dies
# before it starts listening, instead of letting the test fail later with a
# confusing diff.
#
# Usage: wait_for_request_handler <pid>
wait_for_request_handler() {
  local pid="$1"

  if ! wait_for_port 8998 30; then
    echo "Request handler (pid ${pid}) never started listening on 127.0.0.1:8998" >&2
    kill "$pid" 2>/dev/null || true
    exit 1
  fi

  if ! kill -0 "$pid" 2>/dev/null; then
    echo "Request handler (pid ${pid}) exited before it started listening" >&2
    exit 1
  fi
}

# Wait until a file contains at least <count> lines.
#
# This replaces fixed post-request sleeps: it returns as soon as the expected
# output has arrived (making the suite faster) and tolerates a slow runner
# (making it reliable).
#
# Usage: wait_for_lines <file> <count> [timeout_seconds]
wait_for_lines() {
  local file="$1"
  local count="$2"
  local timeout="${3:-30}"
  local deadline=$((SECONDS + timeout))

  while (( SECONDS < deadline )); do
    if [[ -f "$file" ]] && (( $(wc -l < "$file") >= count )); then
      return 0
    fi
    sleep 0.05
  done

  echo "Timed out after ${timeout}s waiting for ${count} line(s) in ${file}" >&2
  return 1
}

# Stop the request handler and wait for the port to be released.
#
# Harnesses run back to back and all bind 127.0.0.1:8998. A bare `kill` only
# sends the signal: the process may still be alive (and holding the port) when
# the next harness starts its own handler, which then aborts with a bind
# conflict and fails the test. Waiting for the process to exit and for the port
# to become free removes that race.
#
# Usage: stop_request_handler <pid>
stop_request_handler() {
  local pid="$1"

  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true

  local deadline=$((SECONDS + 10))
  while (( SECONDS < deadline )); do
    if ! (exec 3<>"/dev/tcp/127.0.0.1/8998") 2>/dev/null; then
      return 0
    fi
    exec 3<&- 3>&-
    sleep 0.05
  done

  echo "Port 8998 is still in use after stopping request handler (pid ${pid})" >&2
  return 1
}
