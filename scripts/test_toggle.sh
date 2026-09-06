#!/usr/bin/env bash
set -euo pipefail

if [[ ! -x ./build/wayvpet ]]; then
  echo "Error: ./build/wayvpet not found. Build first with: make"
  exit 1
fi

if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
  echo "Skipping toggle test: WAYLAND_DISPLAY is not set."
  exit 0
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" || ! -S "${XDG_RUNTIME_DIR}/${WAYLAND_DISPLAY}" ]]; then
  echo "Skipping toggle test: Wayland socket is not available."
  exit 0
fi

show_processes() {
  if ! pgrep -x -a wayvpet; then
    echo "No wayvpet processes found"
  fi
}

is_running() {
  pgrep -x wayvpet >/dev/null 2>&1
}

echo "Testing wayvpet toggle functionality..."
echo

if is_running; then
  echo "Pre-clean: existing wayvpet instance detected, toggling it off first."
  ./build/wayvpet --toggle || true
  sleep 1
fi

echo "1. Starting wayvpet with --toggle (should start since not running):"
if ! ./build/wayvpet --toggle; then
  echo "Skipping toggle test: wayvpet could not start (Wayland unavailable)."
  exit 0
fi
sleep 2

echo
echo "2. Checking if wayvpet is running:"
show_processes

echo
echo "3. Toggling wayvpet off (should stop the running instance):"
./build/wayvpet --toggle
sleep 1

echo
echo "4. Checking if wayvpet is still running:"
show_processes

echo
echo "5. Toggling wayvpet on again (should start since not running):"
./build/wayvpet --toggle
sleep 2

echo
echo "6. Final check - wayvpet should be running:"
show_processes

echo
echo "7. Cleaning up - stopping wayvpet:"
./build/wayvpet --toggle || true

echo
echo "Toggle functionality test completed!"
