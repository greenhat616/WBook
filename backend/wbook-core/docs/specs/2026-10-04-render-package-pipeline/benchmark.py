import json
import platform
import subprocess
import sys
import time
from pathlib import Path

import psutil


def main():
    executable = Path(sys.argv[1]).resolve()
    output = Path(sys.argv[2]).resolve()
    output.mkdir(parents=True, exist_ok=False)
    results = []
    for size in (10, 100, 300):
        for layout in ("split", "paged", "single"):
            process = subprocess.Popen(
                [str(executable), str(output / f"{size}-{layout}"), str(size), layout],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )
            monitor = psutil.Process(process.pid)
            peak = 0
            while process.poll() is None:
                try:
                    memory = monitor.memory_info()
                    peak = max(peak, getattr(memory, "peak_wset", memory.rss))
                except psutil.NoSuchProcess:
                    pass
                time.sleep(0.02)
            stdout, stderr = process.communicate()
            if process.returncode:
                raise RuntimeError(stderr)
            row = json.loads(stdout)
            row["peak_working_set_bytes"] = peak
            results.append(row)
            print(json.dumps(row), flush=True)
            (output / "results.json").write_text(
                json.dumps({"platform": platform.platform(), "results": results}, indent=2),
                encoding="utf-8",
            )


if __name__ == "__main__":
    main()
