#!/bin/sh -xe

python3 -m venv .venv

source .venv/bin/activate

python -m pip install python-osc

python kiyo_zoom.py --kiyoctl ./target/release/kiyoctl
