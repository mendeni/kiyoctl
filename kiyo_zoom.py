#!/usr/bin/env python3

import argparse
import re
import subprocess
import threading
import time

from pythonosc.dispatcher import Dispatcher
from pythonosc.osc_server import ThreadingOSCUDPServer

MIN_ZOOM = 100
MAX_ZOOM = 400

UPDATE_INTERVAL = 0.15
INITIAL_STEP = 3
MAX_STEP = 5
ACCELERATION_DELAY = 0.6

zoom_in = False
zoom_out = False
pressed_at = None

lock = threading.Lock()


def run_kiyoctl(executable, *args):
    return subprocess.run(
        [executable, *args],
        capture_output=True,
        text=True,
        check=True,
        timeout=3
    )


def read_zoom_range(executable):
    result = run_kiyoctl(executable, "probe")

    match = re.search(
        r"zoom\s*=\s*(\d+)\s+\(min\s+(\d+)\s+max\s+(\d+)",
        result.stdout
    )

    if not match:
        raise RuntimeError("Could not read Zoom range from camera.")

    current, minimum, maximum = map(int, match.groups())
    return current, minimum, maximum


def zoom_in_handler(address, value):
    global zoom_in, pressed_at

    with lock:
        new_state = bool(value)

        if new_state and not zoom_in:
            pressed_at = time.monotonic()

        zoom_in = new_state


def zoom_out_handler(address, value):
    global zoom_out, pressed_at

    with lock:
        new_state = bool(value)

        if new_state and not zoom_out:
            pressed_at = time.monotonic()

        zoom_out = new_state


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--kiyoctl",
        default="./kiyoctl",
        help="Path to kiyoctl executable"
    )
    parser.add_argument("--port", type=int, default=9000)
    args = parser.parse_args()

    current_zoom, minimum, maximum = read_zoom_range(args.kiyoctl)

    print(f"Kiyo Pro connected. Zoom: {current_zoom}")
    print(f"Available range: {minimum}–{maximum}")
    print(f"Listening for OSC on port {args.port}")

    dispatcher = Dispatcher()
    dispatcher.map("/zoom/in", zoom_in_handler)
    dispatcher.map("/zoom/out", zoom_out_handler)

    server = ThreadingOSCUDPServer(
        ("127.0.0.1", args.port),
        dispatcher
    )

    server_thread = threading.Thread(
        target=server.serve_forever,
        daemon=True
    )
    server_thread.start()

    global zoom_in, zoom_out, pressed_at

    try:
        while True:
            with lock:
                up = zoom_in
                down = zoom_out
                held_since = pressed_at

            direction = int(up) - int(down)

            if direction != 0:
                held = (
                    time.monotonic() - held_since
                    if held_since is not None else 0
                )

                step = (
                    MAX_STEP
                    if held >= ACCELERATION_DELAY
                    else INITIAL_STEP
                )

                new_zoom = max(
                    minimum,
                    min(maximum, current_zoom + direction * step)
                )

                if new_zoom != current_zoom:
                    run_kiyoctl(
                        args.kiyoctl,
                        "set",
                        f"zoom={new_zoom}"
                    )

                    current_zoom = new_zoom
                    print(f"Zoom: {current_zoom}")

            time.sleep(UPDATE_INTERVAL)

    except KeyboardInterrupt:
        print("\nStopping zoom controller.")

    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()

