# caffeinate

Cross-platform `caffeinate` for Linux and Windows with macOS-compatible CLI flags.

macOS already ships `caffeinate` at `/usr/bin/caffeinate`; this project brings
the same command and flag semantics to Linux and Windows. On macOS this binary
refuses to run so it can never shadow the real one.

## What it does

`caffeinate` prevents sleep while you run a command or until timeout/PID completion.

Supported flags:

- `-d` prevent display sleep
- `-i` prevent idle sleep
- `-m` prevent disk-idle sleep (mapped to idle inhibition on Linux/Windows)
- `-s` prevent system sleep
- `-u` declare user active (defaults to `5` seconds if `-t` is omitted)
- `-t <seconds>` timeout
- `-w <pid>` wait for PID to exit

Behavior matches macOS `caffeinate` rules:

- Default assertion is `-i` when no assertion flags are given.
- `-t` is ignored when a utility command is provided.
- `-w` is ignored when a utility command is provided.

`-s` is AC-power-only on Linux/Windows (best effort from host power status).

## Parity status

The test suite enforces macOS-compatible CLI semantics for:

- option parsing (`-d -i -m -s -u -t -w`, combined flags, attached values)
- default `-i` behavior
- `-u` default 5-second timeout
- `-t`/`-w` ignored when a utility command is provided
- timeout and PID wait runtime behavior

Platform sleep-prevention APIs are mapped to Linux/Windows equivalents. Physical power/sleep behavior still depends on host OS policy.

## Usage

```sh
caffeinate [-disum] [-t timeout] [-w pid] [utility [argument ...]]
```

Examples:

```sh
caffeinate make test
caffeinate -disu -t 3600
caffeinate -w 12345
```

## Install

### Windows (Scoop)

```powershell
scoop bucket add rocky https://github.com/i-rocky/scoop-bucket
scoop install caffeinate
```

### Linux (Homebrew tap)

```sh
brew tap i-rocky/tap
brew install caffeinate
```

### Manual

Download the latest archive from GitHub Releases and put `caffeinate` (`caffeinate.exe` on Windows) on your `PATH`.

## Build

```sh
cargo build --release
```

## Release assets

Tagging `v*` triggers GitHub Actions release publishing with:

- `caffeinate-windows-x86_64-vX.Y.Z.zip`
- `caffeinate-linux-x86_64-vX.Y.Z.tar.gz`
- `caffeinate-linux-aarch64-vX.Y.Z.tar.gz`
- `SHA256SUMS.txt`

These filenames are used by Scoop and Homebrew auto-updaters.
