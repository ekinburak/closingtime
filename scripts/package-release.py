#!/usr/bin/env python3
"""Build a native prototype archive and checksum without installing or publishing."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


def run(*args, cwd):
    return subprocess.run(args, cwd=cwd, check=True, text=True, stdout=subprocess.PIPE).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="Use cached Cargo dependencies only")
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    host = next(line.removeprefix("host: ") for line in run("rustc", "-vV", cwd=root).splitlines() if line.startswith("host: "))
    if not (host.endswith("apple-darwin") or "linux" in host):
        parser.error("Only native Linux and macOS builds are supported")
    flags = ["--locked"] + (["--offline"] if args.offline else [])
    metadata = json.loads(run("cargo", "metadata", "--no-deps", "--format-version", "1", *flags, cwd=root))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "closingtime")
    subprocess.run(["cargo", "build", "--release", "--target", host, "--package", "closingtime", *flags], cwd=root, check=True)
    binary = Path(metadata["target_directory"]) / host / "release" / "closingtime"
    expected = f"closingtime {version}"
    if run(str(binary), "--version", cwd=root).strip() != expected:
        raise SystemExit("Built binary version does not match package metadata")
    name = f"closingtime-{version}-{host}"
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{name}.tar.gz"
    checksum = output / f"{name}.tar.gz.sha256"
    if archive.exists() or checksum.exists():
        raise SystemExit(f"Refusing to replace an existing package: {archive}")
    with tempfile.TemporaryDirectory(prefix="closingtime-package-") as temp:
        package = Path(temp) / name
        package.mkdir()
        shutil.copy2(binary, package / "closingtime")
        (package / "closingtime").chmod(0o755)
        shutil.copy2(root / "LICENSE", package / "LICENSE")
        shutil.copytree(root / "docs", package / "docs")
        (package / "INSTALL.md").write_text(f"""# Closingtime {version} — local prototype

Native target: `{host}`. Built and smoke-checked on the packaging host.
This archive is unsigned, not a public release, and has not completed the
public-alpha gates in the source project's TODO.md. It does not establish
compatibility with older macOS versions or a different Linux libc.

No Rust or C toolchain is needed to use the included executable.

Download the archive and its matching .sha256 file into the same directory.
Verify the archive before extracting it:

```sh
shasum -a 256 -c {name}.tar.gz.sha256 # macOS
# Linux: sha256sum -c {name}.tar.gz.sha256
tar -xzf {name}.tar.gz
./{name}/closingtime --version
./{name}/closingtime doctor
```

To install for your user, after checking whether an older copy exists:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 {name}/closingtime "$HOME/.local/bin/closingtime"
```

Add `$HOME/.local/bin` to your PATH if necessary, or use the full binary path.
The checksum detects corruption; it does not authenticate the publisher.

Wrap a run with `closingtime run -- claude` or `closingtime run -- codex`.
Cleanup always starts with a preview and requires terminal review.
Read docs/guide.md for platform-specific limits before applying cleanup.
""")
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(package, arcname=name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum.write_text(f"{digest}  {archive.name}\n")
    print(archive)
    print(checksum)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from None
