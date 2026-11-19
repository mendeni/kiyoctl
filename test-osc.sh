#!/bin/sh
set -eu

# simple script to excercise the gimball
# Install oscsend with: brew install liblo
# Enable the OSC listener in Preferences; change the port below if needed.

oscsend localhost 9000 /zoom/out i 1
sleep 1
oscsend localhost 9000 /zoom/out i 0
sleep 1
oscsend localhost 9000 /zoom/out i 1
sleep 1
oscsend localhost 9000 /zoom/out i 0
sleep 1
