"""Repeatable Linux assembly/IR benchmarks with verified outputs and GNU time RSS."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PHASES = ("asm-tokenize", "asm-parse", "asm-encode", "asm-object", "asm-library-e2e",
          "ir-parse-verify", "ir-verify", "ir-print", "ir-library-e2e")
FORMAT_VERSION = 1
GENERATOR = "scalar-add-zero-v1"


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def text_command(command, cwd=ROOT):
    return subprocess.run(command, cwd=cwd, check=True, capture_output=True,
                          text=True, timeout=30).stdout.strip()


def fingerprint(record):
    return {key: record[key] for key in ("phase", "count", "input_bytes", "output_bytes", "result_digest", "correct")}


def validate_record(record, phase, count):
    if record.get("phase") != phase or type(record.get("count")) is not int or record.get("count") != count or record.get("correct") is not True:
        raise ValueError("probe did not confirm the requested workload and correctness")
    for key in ("input_bytes", "output_bytes", "elapsed_ns"):
        if type(record.get(key)) is not int or record[key] < 0:
            raise ValueError(f"invalid probe {key}")
    if not isinstance(record.get("result_digest"), str) or len(record["result_digest"]) != 16:
        raise ValueError("invalid probe digest")
    return record


def measurement_index(report, label):
    measurements = {}
    for entry in report["measurements"]:
        name = entry["name"]
        if type(name) is not str or not name:
            raise ValueError(f"{label} has an invalid measurement name")
        if name in measurements:
            raise ValueError(f"{label} has a duplicate measurement: {name}")
        measurements[name] = entry
    return measurements


def compare_reports(previous, current):
    # Timing comparisons need identical workloads/build mode/toolchain/host.
    # Different revisions and executable hashes are intentional, other changes
    # would confound a regression comparison and must be measured separately.
    for key in ("format_version", "generator", "profile", "rustc", "host", "build_environment", "warmups", "runs", "counts"):
        if previous.get(key) != current.get(key):
            raise ValueError(f"comparison requires matching {key}")
    workload_keys = ("size", "count", "sha256")
    before_inputs = [{key: item[key] for key in workload_keys} for item in previous.get("workloads", [])]
    after_inputs = [{key: item[key] for key in workload_keys} for item in current.get("workloads", [])]
    if before_inputs != after_inputs:
        raise ValueError("comparison requires identical input and expected artifact hashes")
    old = measurement_index(previous, "baseline")
    new = measurement_index(current, "current report")
    if old.keys() != new.keys():
        raise ValueError("comparison requires the same phases")
    comparison = {}
    for name, entry in new.items():
        if old[name]["fingerprint"] != entry["fingerprint"]:
            raise ValueError(f"comparison correctness/output fingerprint differs: {name}")
        before = old[name]["median"]
        after = entry["median"]
        comparison[name] = {
            "phase_time_ratio": after["phase_seconds"] / before["phase_seconds"] if before["phase_seconds"] else None,
            "rss_ratio": after["peak_rss_bytes"] / before["peak_rss_bytes"] if before["peak_rss_bytes"] else None,
        }
    return comparison


def execute(command, timeout, timer=None, cwd=ROOT, env=None):
    """One private POSIX process group; timeout never leaves owned children."""
    with tempfile.TemporaryDirectory(prefix="whale benchmark ") as directory:
        rss_path = Path(directory) / "rss"
        launched = [str(timer), "-f", "%M", "-o", str(rss_path), "--", *map(str, command)] if timer else list(map(str, command))
        started = time.perf_counter()
        process = subprocess.Popen(launched, cwd=cwd, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=True, env=env)
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.communicate()
            raise ValueError(f"benchmark command exceeded {timeout}s: {command}") from None
        finally:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        elapsed = time.perf_counter() - started
        if process.returncode:
            raise ValueError(f"command exited {process.returncode}: {command}\n{stderr.decode(errors='replace')[-8192:]}")
        peak = int(rss_path.read_text().strip()) * 1024 if timer else None
        return stdout, elapsed, peak


def summarize(samples):
    return {key: statistics.median(sample[key] for sample in samples)
            for key in ("phase_seconds", "process_wall_seconds", "peak_rss_bytes")}


def sample_phase(name, count, command, runs, warmups, timeout, timer, validator, before=None):
    samples = []
    expected = None
    for number in range(warmups + runs):
        if before:
            before()
        stdout, wall, peak = execute(command, timeout, timer)
        record = validator(stdout, wall)
        current = fingerprint(record)
        if expected is not None and current != expected:
            raise ValueError(f"non-deterministic or incorrect output for {name}")
        expected = current
        if number >= warmups:
            samples.append({"phase_seconds": record["elapsed_ns"] / 1e9,
                            "process_wall_seconds": wall, "peak_rss_bytes": peak})
    return {"name": name, "command": list(map(str, command)), "count": count,
            "fingerprint": expected, "samples": samples, "median": summarize(samples)}


def cli_validator(phase, count, source, artifact, expected):
    def validate(_stdout, wall):
        if not artifact.is_file() or sha256(artifact) != sha256(expected):
            raise ValueError(f"CLI artifact does not match the validated fixture: {phase}")
        return {"phase": phase, "count": count, "input_bytes": source.stat().st_size,
                "output_bytes": artifact.stat().st_size, "elapsed_ns": round(wall * 1e9),
                "result_digest": sha256(artifact)[:16], "correct": True}
    return validate


def host_metadata():
    cpu = platform.processor()
    info = Path("/proc/cpuinfo")
    if info.exists():
        for line in info.read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    return {"platform": platform.platform(), "machine": platform.machine(), "cpu": cpu,
            "logical_cpus": os.cpu_count()}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, default=Path("target/benchmark"))
    parser.add_argument("--profile", choices=("release", "debug"), default="release")
    parser.add_argument("--small", type=int, default=64)
    parser.add_argument("--large", type=int, default=32768)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--library-only", action="store_true", help="explicitly skip both real CLI measurements")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--probe", type=Path)
    parser.add_argument("--cli", type=Path)
    args = parser.parse_args(argv)
    if not (0 <= args.small < args.large <= 100000 and 1 <= args.runs <= 100 and 0 <= args.warmups <= 20 and math.isfinite(args.timeout) and args.timeout > 0):
        parser.error("require 0 <= small < large <= 100000, runs1..100, warmups0..20, timeout>0")
    timer = Path("/usr/bin/time")
    if not sys.platform.startswith("linux") or not timer.is_file():
        parser.error("this benchmark requires Linux and GNU /usr/bin/time for comparable peak RSS")
    try:
        timer_version = text_command([str(timer), "--version"]).splitlines()[0]
        if "GNU" not in timer_version and "time (" not in timer_version:
            raise ValueError("/usr/bin/time must provide GNU %M in KiB")
        target = args.target_dir.resolve()
        build = ["cargo", "build", "--locked", "--offline", "--jobs", "2", "--target-dir", str(target), "--example", "benchmark_probe"]
        if args.profile == "release":
            build.append("--release")
        if not args.library_only:
            build += ["--bin", "whale"]
        if not args.skip_build:
            execute(build, 600, env=dict(os.environ, CARGO_INCREMENTAL="0"))
        probe = (args.probe or target / args.profile / "examples/benchmark_probe").resolve(strict=True)
        cli = None if args.library_only else (args.cli or target / args.profile / "whale").resolve(strict=True)
        build_environment = {key: os.environ.get(key) for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTUP_TOOLCHAIN", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")}
        build_environment.update({key: value for key, value in os.environ.items() if key.startswith("CARGO_PROFILE_")})
        build_environment.update(CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="2")
        report = {"format_version": FORMAT_VERSION, "generator": GENERATOR,
                  "revision": text_command(["git", "rev-parse", "HEAD"]),
                  "dirty": bool(text_command(["git", "status", "--porcelain"])),
                  "rustc": text_command(["rustc", "-vV"]), "cargo": text_command(["cargo", "-V"]),
                  "python": platform.python_version(), "host": host_metadata(), "build_environment": build_environment,
                  "profile": args.profile, "build_command": build, "build_executed": not args.skip_build,
                  "runs": args.runs, "warmups": args.warmups, "counts": [args.small, args.large],
                  "rss_method": timer_version + "; %M KiB converted to bytes; whole child-process high-water mark",
                  "probe_sha256": sha256(probe), "cli_sha256": sha256(cli) if cli else None,
                  "probe_source_sha256": sha256(ROOT / "examples/benchmark_probe.rs"),
                  "driver_source_sha256": sha256(Path(__file__)),
                  "binaries_built_by_command": not args.skip_build and args.probe is None and (cli is None or args.cli is None),
                  "cli_measured": cli is not None, "measurements": [], "workloads": []}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        inputs = args.output.parent / (args.output.stem + "-inputs")
        for size, count in (("small", args.small), ("large", args.large)):
            directory = (inputs / size).resolve()
            prepare = [str(probe), "prepare", str(count), str(directory)]
            data, _, _ = execute(prepare, args.timeout)
            validate_record(json.loads(data), "prepare", count)
            report["workloads"].append({"size": size, "count": count, "prepare_command": prepare,
                "sha256": {name: sha256(directory / name) for name in ("input.asm", "input.wir", "expected.o")}})
            for phase in PHASES:
                command = [str(probe), phase, str(count)]
                validator = lambda stdout, _wall, p=phase, n=count: validate_record(json.loads(stdout), p, n)
                entry = sample_phase(f"{size}/{phase}", count, command, args.runs, args.warmups, args.timeout, timer, validator)
                report["measurements"].append(entry)
            if cli:
                for phase, command, source, artifact, expected in [
                    ("asm-cli-e2e", [str(cli), "asm", "--amd64", str(directory / "input.asm"), "-o", str(directory / "actual.o")], directory / "input.asm", directory / "actual.o", directory / "expected.o"),
                    ("ir-cli-e2e", [str(cli), "ir", "print", str(directory / "input.wir"), "-o", str(directory / "actual.wir")], directory / "input.wir", directory / "actual.wir", directory / "input.wir"),
                ]:
                    report["measurements"].append(sample_phase(f"{size}/{phase}", count, command, args.runs, args.warmups, args.timeout, timer, cli_validator(phase, count, source, artifact, expected), before=lambda path=artifact: path.unlink(missing_ok=True)))
        if args.compare:
            report["comparison"] = compare_reports(json.loads(args.compare.read_text()), report)
        # Publish only a complete successful report, preserving an earlier report
        # if a command, correctness check or comparison fails.
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=args.output.parent, delete=False) as stream:
            temporary = Path(stream.name)
            try:
                json.dump(report, stream, indent=2)
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
                stream.close()
                os.replace(temporary, args.output)
            finally:
                temporary.unlink(missing_ok=True)
        for entry in report["measurements"]:
            median = entry["median"]
            print(f"{entry['name']}: {median['phase_seconds'] * 1000:.3f} ms; {median['peak_rss_bytes'] / 1048576:.2f} MiB RSS; {entry['fingerprint']['output_bytes']} output bytes")
        print(f"saved {args.output}; CLI measured: {report['cli_measured']}")
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"benchmark failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
