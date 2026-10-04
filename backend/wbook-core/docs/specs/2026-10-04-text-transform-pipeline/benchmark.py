import argparse
import json
from pathlib import Path
import platform
import statistics
import subprocess

import psutil


def measure(binary, mode, workload, mib):
    process = subprocess.Popen(
        [str(binary), mode, workload, str(mib), "hold"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    try:
        line = process.stdout.readline()
        if not line:
            raise RuntimeError(process.stderr.read())
        result = json.loads(line)
        memory = psutil.Process(process.pid).memory_info()
        result["peak_mib"] = memory.peak_wset / (1024 * 1024)
        process.communicate("\n", timeout=30)
        if process.returncode:
            raise RuntimeError(f"benchmark exited with {process.returncode}")
        return result
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if platform.system() != "Windows":
        parser.error("this sampler uses Windows lifetime peak working set")
    if args.repeats < 1:
        parser.error("repeats must be positive")
    rows = []
    for workload in ["sparse", "dense", "repeated", "longline"]:
        for mib in [10, 100, 300]:
            pair = []
            for mode in ["pieces", "string"]:
                runs = [measure(args.binary.resolve(), mode, workload, mib) for _ in range(args.repeats)]
                assert len({(r["hash"], r["output_bytes"]) for r in runs}) == 1
                row = dict(runs[0])
                for field in ["edit_ms", "scan_ms", "peak_mib"]:
                    row[field] = statistics.median(r[field] for r in runs)
                row["runs"] = [{field: run[field] for field in ["edit_ms", "scan_ms", "peak_mib"]} for run in runs]
                pair.append(row)
                rows.append(row)
                print(f"{workload} {mib} MiB {mode}: edit {row['edit_ms']:.2f} ms, scan {row['scan_ms']:.2f} ms, peak {row['peak_mib']:.1f} MiB", flush=True)
            assert (pair[0]["hash"], pair[0]["output_bytes"]) == (pair[1]["hash"], pair[1]["output_bytes"])
    args.output.write_text(json.dumps({
        "platform": platform.platform(),
        "logical_cpus": psutil.cpu_count(),
        "physical_memory_gib": psutil.virtual_memory().total / (1024 ** 3),
        "repeats": args.repeats,
        "rows": rows,
    }, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
