# Prototype binary preparation

Public binaries and Homebrew are the next distribution priority. No package or
release has been published. Preparing an archive does not pass the public-alpha
gates in [TODO.md](../TODO.md).

Build a native archive on Linux or macOS using Python 3.9+ and the existing Rust/C
build tools:

```sh
python3 scripts/package-release.py
# With previously cached dependencies:
python3 scripts/package-release.py --offline
```

The script builds the locked CLI, checks its version, and packages the executable,
license, safety documentation and install instructions in `dist/`, alongside a
SHA-256 checksum. It refuses to overwrite an existing package. Recipients do not
need Rust or a C toolchain. Archives are unsigned and native to the packaging
host; older macOS and different Linux libc compatibility are not established.

The manual `Prepare prototype binaries` workflow runs native tests and terminal
checks on the existing Linux/macOS CI targets and uploads review artifacts. It
does not create tags, GitHub Releases, a Homebrew tap or website downloads.
The workflow has not been executed in a hosted repository.

Before offering public downloads: finish live-agent validation, the hosted
platform checks and macOS cleanup decision; choose supported OS/architectures;
verify freshly extracted archives on those systems; arrange release signing and
macOS notarization; publish immutable assets and checksums. Then build the
Homebrew formula against those actual asset URLs and hashes, test its install,
and update the website. No placeholder `brew install` command is advertised.
