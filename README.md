# rustcounter — an atomic counter Linux kernel module

A Linux kernel module demonstrating Rust's compiler-enforced data-race prevention. Concurrent writes through `/dev/rustcounter` are aggregated by an `AtomicU64` — thousands of parallel writes always produce the exact total, with no locks and no scheduler involvement.

## What It Does

This module creates a character device `/dev/rustcounter` that maintains a global atomic counter (`AtomicU64`). Each write increments it atomically; each read returns the current value as ASCII. The counter is lock-free — the hardware provides atomic read-modify-write operations, and Rust's type system ensures you can't accidentally create a data race.

**Why this matters:** In C, writing `count++` without a lock creates a data race that silently loses increments under concurrent load. In Rust, the compiler won't let you compile that code — you must either use an `AtomicU64` (correct and fast) or wrap the counter in a `Mutex` (correct but slower). This module demonstrates the atomic path.

## Demo

```bash
# Build and load
make
sudo insmod rustcounter.ko

# Initial read
sudo cat /dev/rustcounter
# → 0

# Single increment
echo bump | sudo tee /dev/rustcounter > /dev/null
sudo cat /dev/rustcounter
# → 1

# Stress test: two terminals, 1000 writes each
# Terminal 1:
for i in {1..1000}; do echo x | sudo tee /dev/rustcounter > /dev/null; done

# Terminal 2 (run simultaneously):
for i in {1..1000}; do echo y | sudo tee /dev/rustcounter > /dev/null; done

# After both finish:
sudo cat /dev/rustcounter
# → 2000  (always exact, never loses increments)

# Unload
sudo rmmod rustcounter
```

The final count is **always 2000**, every time. Equivalent C code with `count++` and no lock would lose increments — sometimes a few, sometimes hundreds, depending on CPU timing. Rust makes that bug uncodeable.

## Build Instructions

### Prerequisites

Running on **Ubuntu 26.04 LTS** with a Rust-enabled kernel (kernel 7.0+):

```bash
sudo apt update
sudo apt install -y build-essential linux-headers-$(uname -r) kmod
sudo apt install -y rustc-1.93 rust-1.93-src bindgen
sudo update-alternatives --install /usr/bin/rustc rustc /usr/bin/rustc-1.93 100
```

Verify setup:

```bash
uname -r              # should show 7.0.0-14-generic or newer
rustc --version       # should show 1.93.x
ls /lib/modules/$(uname -r)/build/rust  # should exist
```

### Build

```bash
make
```

This produces `rustcounter.ko` (the kernel module).

### Load and Test

```bash
sudo insmod rustcounter.ko
lsmod | grep rustcounter
ls -l /dev/rustcounter   # device node (mode 0600 root:root)

sudo cat /dev/rustcounter
echo test | sudo tee /dev/rustcounter > /dev/null
sudo cat /dev/rustcounter

sudo dmesg | tail -5     # kernel log shows increments
sudo rmmod rustcounter
```

## Code Tour

**rustcounter.rs** — the module (75 lines)

1. **Static state** (lines 19-20):
   - `COUNT: AtomicU64` — the global counter, initialized to 0
   - `CONSUMED: AtomicBool` — EOF flag (prevents `cat` from looping forever)

2. **Module structure** (lines 23-38):
   - `RustCounter` — the module itself, holds the device registration
   - `impl InPlaceModule::init` — called when the module loads, registers `/dev/rustcounter`
   - Uses the 2026 Rust-for-Linux API (`kernel::InPlaceModule`, `try_pin_init!`, `KBox`)

3. **Device operations** (lines 40-75):
   - `RustCounterDevice` — per-open state (in this case, none needed — the atomics are global)
   - `open()` — creates a new `KBox<RustCounterDevice>` for each `open()` syscall
   - `write_iter()` — drains user bytes, calls `COUNT.fetch_add(1, Ordering::SeqCst)`, logs the new value
   - `read_iter()` — formats the current count as a string, copies it to userspace, marks it consumed

**Makefile** — hooks into the kernel's Kbuild system for out-of-tree Rust modules. No `Cargo.toml` is used — the kernel's build invokes `rustc` directly with all the special flags Rust-in-the-kernel requires.

## Design Notes

### Why `AtomicU64` instead of `Mutex<u64>`?

A `Mutex` is correct but wasteful for a simple counter. The only operation we need is "read current value and add 1" — the CPU has a single instruction for this (`LOCK XADD` on x86, `ldaddal` on ARM64). An `AtomicU64::fetch_add` compiles down to that one instruction. A `Mutex` adds two memory fences plus potentially a scheduler interaction on contention.

**Rust's type system makes this choice explicit:** you can't accidentally touch a shared counter without synchronization. If you try, the compiler stops you and forces you to pick either an `Atomic` (correct and fast) or a `Mutex` (correct and slower). There is no "I forgot the lock and shipped a data race" path.

### Memory Ordering: `SeqCst`

`Ordering::SeqCst` is the strongest memory ordering — it ensures that all threads see atomic operations in the same total order. For a simple counter you could probably use `Relaxed`, but `SeqCst` is the safe default. If you can't articulate why a weaker ordering is correct, use `SeqCst`.

### EOF Handling

The `CONSUMED` flag implements proper EOF behavior. After each read, it's set to `true` and subsequent reads return 0 (which the VFS layer interprets as EOF). A write resets it to `false`. Without this, `cat /dev/rustcounter` would loop endlessly seeing the same bytes.

### The 2026 API

This module uses the **Ubuntu 26.04 / kernel 7.0** Rust-for-Linux API (`kernel::InPlaceModule`, `KBox`, `try_pin_init!`). Earlier tutorials and LN24's original demo used the 2024 API, which had different boilerplate. Same logic, different structure.

## Future Work

- **Use the kernel RNG** to randomize the initial count (callback to LN23's `/dev/urandom` tracing)
- **Reset command**: write the literal string `RESET\n` to set the counter to 0
- **Per-process counts**: track a `HashMap<pid, AtomicU64>` keyed by the writing process's PID
- **Saturating counter**: clamp at `u64::MAX` instead of overflowing
- **`/proc/rustcounter` view**: expose the count in `/proc` for sysadmin observation without consuming

## License

GPL-2.0 (required for Linux kernel modules)
