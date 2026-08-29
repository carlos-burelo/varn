"""Shared timing harness: warm up, then keep the best of N runs.

Mirrors `benchmarks/js/*.js` exactly — checksum to stderr, best milliseconds
to stdout — so `compare.ps1` can read every runtime the same way. Python gets
fewer repetitions than the JS twins because it is roughly an order of
magnitude slower here and the extra rounds buy nothing.
"""
import sys
import time


def run(compute, warmup=1, runs=3):
    for _ in range(warmup):
        compute()
    best = float("inf")
    chk = None
    for _ in range(runs):
        t = time.perf_counter()
        chk = compute()
        ms = (time.perf_counter() - t) * 1000.0
        if ms < best:
            best = ms
    print(chk, file=sys.stderr)
    print(f"{best:.3f}")
