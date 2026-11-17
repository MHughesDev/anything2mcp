#!/usr/bin/env python3
"""Sits between a client and a server on stdio and writes down every line each sends.

Usage: tee.py <log.jsonl> <server-binary> [args...]

It changes nothing: bytes go through as they came. Standard library only.
"""
import json
import os
import subprocess
import sys
import threading

log_path, argv = sys.argv[1], sys.argv[2:]
child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
lock = threading.Lock()


def note(direction, raw):
    try:
        msg = json.loads(raw)
    except ValueError:
        msg = {"unparsed": raw.decode("utf-8", "replace")}
    with lock, open(log_path, "a", encoding="utf-8") as f:
        f.write(json.dumps({"pid": os.getpid(), "dir": direction, "msg": msg}) + "\n")


def pump(source, sink, direction):
    for raw in iter(source.readline, b""):
        note(direction, raw)
        try:
            sink.write(raw)
            sink.flush()
        except (BrokenPipeError, ValueError):
            break
    try:
        sink.close()
    except (BrokenPipeError, ValueError):
        pass


up = threading.Thread(target=pump, args=(sys.stdin.buffer, child.stdin, "client"), daemon=True)
down = threading.Thread(target=pump, args=(child.stdout, sys.stdout.buffer, "server"))
up.start()
down.start()
down.join()
sys.exit(child.wait())
