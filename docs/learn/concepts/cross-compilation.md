# cross-compilation

A **target** is the system a binary is built for, written as a triple — the processor, the vendor, the system, and sometimes the C library: `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-unknown-linux-musl`. `rustc` can build for any of its targets from any machine. For each it needs:

1. **The standard library built for that target**, which `rustup target add <triple>` downloads.
2. **A linker** that can join that target's object files into a program.
3. And to know it works, **something that can run it**.

`cargo build --target <triple>` puts the result in `target/<triple>/release/`. Checking stops before linking, so `cargo check --target` needs only the first. On Windows, for Linux:

```
$ rustup target add x86_64-unknown-linux-gnu
$ cargo check --all-targets --target x86_64-unknown-linux-gnu
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.64s
$ cargo build --target x86_64-unknown-linux-gnu
error: linker `cc` not found
```

The check compiled the code under `#[cfg(unix)]` — which a build for Windows leaves out, unchecked — and found nothing wrong; the build got as far as the linker this machine does not have.

## The five agentdocs is released for

| target | built on | linker | run before release by |
|---|---|---|---|
| `x86_64-unknown-linux-musl` | Linux, x86_64 | the system's, with the musl start files Rust brings | the runner itself |
| `aarch64-unknown-linux-musl` | Linux, x86_64 | `aarch64-linux-gnu-gcc`, installed for it | `qemu-aarch64-static`, an emulator |
| `aarch64-apple-darwin` | macOS, Apple silicon | Apple's | the runner itself |
| `x86_64-apple-darwin` | macOS, Apple silicon | Apple's, which links for both | Rosetta, Apple's translator |
| `x86_64-pc-windows-msvc` | Windows | Microsoft's | the runner itself |

Which linker for which target is an environment variable named after the target — `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc` — or `linker = …` under `[target.<triple>]` in `.cargo/config.toml`. The release workflow runs every binary before it is packed, and each must say the version of the tag ([[continuous-integration]]).

## musl: one Linux binary for every distribution

A Linux program usually calls into glibc, the C library its distribution ships, and a binary built against a newer glibc will not start on a system with an older one. The `musl` targets link another C library, musl, into the binary itself, which then needs nothing from the system but its kernel. Built on Debian, both ran on Alpine — which has no glibc at all — x86_64 directly and aarch64 under qemu:

```
$ file agentdocs-aarch64-unknown-linux-musl
ELF 64-bit LSB executable, ARM aarch64, version 1 (SYSV), statically linked
/ # uname -m; ./agentdocs-aarch64-unknown-linux-musl --version
aarch64
agentdocs 0.1.0
```

Nothing in agentdocs is written in C — M8 chose syntect's Rust regex engine to keep a C compiler out of the build ([[external-crates]]) — so nothing else had to be compiled for each target.

## Windows: the C runtime linked in

Built with the defaults, the Windows binary loads `vcruntime140.dll`, which comes with the Visual C++ redistributable and is not on every Windows. `-C target-feature=+crt-static` links the C runtime into the file instead; then it loads only DLLs every Windows has. Set in `.cargo/config.toml` for that one target, so that a build made here is the one released:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

The file grew from 3.60 MB to 3.70 MB.

## `cfg`: one source, a different program for each target

`#[cfg(unix)]` and `#[cfg(windows)]` decide while compiling which code is in the program at all ([[processes]]). What one target leaves out is neither compiled nor checked when building for another: M11's `hold`, which makes a path unreadable for a test, has a Windows half and a Unix half, and the Unix half was first compiled in a Linux container. What then differed when it ran is [[platform-differences]].

## Related

[[release-profiles]] · [[platform-differences]] · [[continuous-integration]] · [[external-crates]] · [[processes]]
