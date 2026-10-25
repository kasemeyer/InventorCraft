# Building TCI-CAD

TCI-CAD is a fork of [SolidCraft](https://github.com/tangency-labs/solidcraft). Upstream's own
build notes in `docs/building.md` still apply. This file records what is specific to TCI-CAD and
to the machines it is developed on. Linux and Windows are both first-class targets: the goal is
Inventor-like CAD that is not tied to an operating system.

## Build hosts

| Host | Role | Status |
|---|---|---|
| Windows 11 (32 cores, 64 GB) | Primary for UI-fidelity and Inventor-interop work; Inventor 2027 is installed here for side-by-side comparison | Native build, see below |
| Linux (Omarchy: Arch + Hyprland) | Equal host for kernel-adjacent and document-model work | Set up with `scripts/bootstrap-linux.sh` in the planning repo |

The plan's fallback to WSL was not needed: the gate runs natively on Windows.

## Native Windows setup (done 2026-10-24)

- **Rust** via rustup, `stable-x86_64-pc-windows-msvc` (1.99.1 at the time of writing; upstream
  needs 1.95+). If `cargo --version` says the cargo binary "is not applicable to the toolchain",
  the toolchain is corrupt: `rustup toolchain uninstall stable && rustup toolchain install stable`.
- **MSVC linker:** Visual Studio 2026 Build Tools with the C++ workload:
  `winget install --id Microsoft.VisualStudio.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`.
  Without it rustc picks up Git's coreutils `link.exe` and every link fails.
- **wasm32 target** for the gate's browser check: `rustup target add wasm32-unknown-unknown`.
- **Python 3.11+** is called as `python3` by the icons gate. On Windows that name resolves to the
  Microsoft Store stub, so either disable the `python3` app-execution alias or copy
  `python.exe` to `python3.exe` inside the real Python install directory.
- **Chrome** is found automatically by the offline web test; without it the test is skipped.

## Keep the desktop usable during builds

A cold build with one rustc per core made the machine unusable. Cap jobs in the user-level
cargo config (`%USERPROFILE%\.cargo\config.toml`, not the repo's, so upstream is not diverged):

```toml
[build]
jobs = 12
```

and start long runs at below-normal priority, for example from PowerShell:

```powershell
$p = Start-Process pwsh -ArgumentList '-NoProfile','-Command','cargo xtask ci' -PassThru
$p.PriorityClass = 'BelowNormal'
```

## The gate

`cargo xtask ci` is the gate: fmt, asset provenance, icons, licences, lists, layering, UI
containers, clippy with `-D warnings`, the wasm32 check, tests, doc tests, and the Chrome
offline web test. Upstream only runs clippy on Linux, so Windows-only dead code can slip into
upstream `main` (it did on 2026-10-24 in `space_mouse.rs`, fixed the same day). If clippy fails
on a cfg-gated Windows path right after an upstream merge, check upstream first before
patching.

Baseline gate result on Windows (2026-10-24/25, upstream d5eca6cc): fmt, asset provenance,
icons, licences, lists, layering, UI containers and clippy all green. Unit tests: 77 test
binaries passed; the kernel binary passed 873 tests and failed one, `heal::tests::
long_collinear_runs_join_whole`, which is a wall-clock bound ("took 44.8s") tripped while the
machine was saturated, not a geometry failure. Doc tests and the wasm32 check failed only
because a concurrent edit (Task 2's new `kind` module) was compiled mid-run, so those two
steps are unproven here. The run was stopped after two hours while the `solidcraft-gpu` test
binary was at 23 GB of RAM on the default `SOLIDCRAFT_GPU` policy. The web test was not
reached.

### Run the gate the way upstream's Windows CI does

Upstream's Windows job pins the GPU stack to the software rasterizer and bounds test
parallelism. Use the same environment here, or the gpu tests run for hours:

```powershell
$env:WGPU_BACKEND = "dx12"
$env:WGPU_DX12_COMPILER = "fxc"
$env:SOLIDCRAFT_GPU = "software"
$env:SOLIDCRAFT_TEST_JOBS = "8"
cargo xtask ci
```

Expect the full gate to take well over an hour on this machine. Run it on a clean, committed
tree with no editor or agent touching the sources, or the doc-test and wasm steps compile
half-written code.
