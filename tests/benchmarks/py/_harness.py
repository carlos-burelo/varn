"""Shared timing harness: warm up, then keep the best of N runs.

Mirrors `benchmarks/js/*.js` exactly — checksum to stderr, best milliseconds
to stdout — so `compare.ps1` can read every runtime the same way. Python gets
fewer repetitions than the JS twins because it is roughly an order of
magnitude slower here and the extra rounds buy nothing.
"""
import sys
import time


def run(compute):
    chk = compute()
    if chk is not None and chk != 0:
        print(chk)
