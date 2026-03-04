---
title: "Chapter 6: Testing VHDL with Rust"
date: 2026-03-04
summary: "The skalp Testbench API — set, clock, expect, get_u64, export_waveform. Write async Rust tests for your VHDL designs without ModelSim, without license servers, just cargo test."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 6
ShowToc: true
---

## What This Chapter Teaches

This is the payoff. Everything in the first five chapters — counters, muxes, FSMs, generic register banks, hierarchical bus designs — was building toward this moment. You now have real VHDL designs, and this chapter shows you how to test every one of them without ModelSim, without VHDL testbench boilerplate, without license servers, and without writing a single line of testbench VHDL.

The entire test workflow is one command:

```bash
cargo test
```

That command compiles your VHDL through skalp, loads each design into an in-process simulator, drives inputs, checks outputs, and reports pass/fail with Rust's standard test runner. If a test fails, you get an error message with the expected value, the actual value, and the signal name. If you need to debug, dump a VCD waveform and open it in GTKWave.

By the end of this chapter you will understand:

- The complete `Testbench` API: `new`, `set`, `clock`, `expect`, `get_u64`, `reset`, `export_waveform`
- How to test the counter from Chapter 1, the timer from Chapter 3, and the I2C FSM from Chapter 3
- How to write helper functions that make tests readable and reusable
- How to dump and inspect waveforms when a test fails
- How skalp's testing approach compares to traditional VHDL and SystemVerilog testbenches

No prior Rust experience is required beyond what the earlier chapters have shown. The test code is straightforward — `set` a value, `clock` some cycles, `expect` an output.

---

## The Testbench API

skalp provides a Rust crate called `skalp_testing` that contains the `Testbench` type. It compiles your VHDL source, loads the design into a cycle-accurate simulator, and gives you methods to interact with it.

### `Testbench::new` — Create a Simulator

```rust
use skalp_testing::Testbench;

let mut tb = Testbench::new("src/counter.vhd").await.unwrap();
```

Takes the VHDL source path. Returns a `Result` — if the VHDL has errors, you get compile diagnostics. Each test gets its own simulator instance. Tests do not share state and run in parallel.

### `set` — Drive an Input

```rust
tb.set("en", 1u8);
tb.set("threshold", 100u32);
```

Drives an input port to a value. The value takes effect on the next clock edge, mirroring real hardware. Accepts any unsigned integer type (`u8`, `u16`, `u32`, `u64`), truncated to the port width. **Not async** — it queues the value immediately.

### `clock` — Advance Time

```rust
tb.clock(1).await;    // advance 1 cycle
tb.clock(100).await;  // advance 100 cycles
```

Runs the simulator for N clock cycles. This is the only way time advances — between `clock` calls the design is frozen. Tests are deterministic: the same sequence of `set` and `clock` calls always produces the same result.

### `expect` — Assert an Output

```rust
tb.expect("count", 10u32).await;
```

Reads a port or internal signal and asserts it equals the expected value. On mismatch:

```
assertion failed: signal 'count' expected 10, got 7
  in test_counter_counts at tests/counter_test.rs:14
```

This is a Rust panic — `cargo test` reports it as a failure with file and line number.

### `get_u64` — Read a Signal

```rust
let value = tb.get_u64("count").await;
```

Returns the current value as `u64`. Use this for control flow — polling a `done` signal, conditional logic based on signal values.

### `reset` — Assert and Release Reset

```rust
tb.reset(2).await;
```

Asserts `rst` high for N cycles, then deasserts it and clocks one more cycle. If your reset port has a different name, use `set` and `clock` manually.

### `export_waveform` — Dump VCD

```rust
tb.export_waveform("build/counter_test.vcd").unwrap();
```

Writes the complete signal history to a VCD file. Open with GTKWave, Surfer, or any VCD viewer.

### Why Everything Is Async

Every method that touches the simulator requires `.await`. The simulator engine runs in a separate C++ thread, communicating with the Rust test through async channels. Multiple testbenches run concurrently in the same process. You do not need to understand Rust async — just add `.await` after every API call except `set`.

---

## Counter Test Suite

The 8-bit counter from Chapter 1 (`src/counter.vhd`) has three ports to exercise: `rst` clears the counter, `en` enables counting, `count` is the output. Create `tests/counter_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_counter_counts() {
    let mut tb = Testbench::new("src/counter.vhd").await.unwrap();
    tb.reset(2).await;
    tb.expect("count", 0u32).await;
    tb.set("en", 1u8);
    for i in 1..=10u32 {
        tb.clock(1).await;
        tb.expect("count", i).await;
    }
}

#[tokio::test]
async fn test_counter_overflow() {
    let mut tb = Testbench::new("src/counter.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("en", 1u8);
    tb.clock(255).await;
    tb.expect("count", 255u32).await;
    tb.clock(1).await;
    tb.expect("count", 0u32).await;
}

#[tokio::test]
async fn test_counter_disable() {
    let mut tb = Testbench::new("src/counter.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("en", 1u8);
    tb.clock(5).await;
    tb.expect("count", 5u32).await;
    tb.set("en", 0u8);
    tb.clock(10).await;
    tb.expect("count", 5u32).await;
}
```

**`test_counter_counts`** — the happy path. After reset, count is 0. Enable counting, advance one cycle at a time, verify each increment.

**`test_counter_overflow`** — after 255 increments, one more wraps to 0. Catches off-by-one errors.

**`test_counter_disable`** — count to 5, disable for 10 cycles, confirm the count holds. Catches designs where `en` is ignored.

Run the tests:

```bash
cargo test
```

```
running 3 tests
test test_counter_counts ... ok
test test_counter_overflow ... ok
test test_counter_disable ... ok

test result: ok. 3 passed; 0 failed; 0 finished in 0.18s
```

All three run in parallel. To run one test: `cargo test test_counter_overflow`. To see output: `cargo test -- --nocapture`.

---

## Timer Test

The timer from Chapter 3 has a prescaler, threshold, and match output. Create `tests/timer_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_timer_match() {
    let mut tb = Testbench::new("src/timer.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("prescaler", 0u8);  // no division
    tb.set("threshold", 10u32);
    tb.set("enable", 1u8);

    // Count up to threshold
    tb.clock(11).await;
    tb.expect("match_out", 1u32).await;
    tb.expect("counter", 0u32).await;  // reset after match
}

#[tokio::test]
async fn test_timer_prescaler() {
    let mut tb = Testbench::new("src/timer.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("prescaler", 1u8);  // divide by 2
    tb.set("threshold", 5u32);
    tb.set("enable", 1u8);

    // With prescaler=1, counter increments every 2 cycles
    tb.clock(10).await;
    tb.expect("counter", 5u32).await;
    tb.expect("match_out", 1u32).await;
}

#[tokio::test]
async fn test_timer_disabled() {
    let mut tb = Testbench::new("src/timer.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("prescaler", 0u8);
    tb.set("threshold", 10u32);
    tb.set("enable", 0u8);

    tb.clock(100).await;
    tb.expect("counter", 0u32).await;
    tb.expect("match_out", 0u32).await;
}
```

**`test_timer_match`** — with no prescaler, the counter increments every cycle. After 11 cycles it reaches 10, `match_out` asserts, and the counter resets.

**`test_timer_prescaler`** — with `prescaler = 1`, the counter increments every 2 clock cycles. After 10 real cycles, the counter should read 5.

**`test_timer_disabled`** — with `enable` low, nothing moves even after 100 cycles.

---

## I2C FSM Test

The I2C controller from Chapter 3 has multiple states and handshaking signals. Create `tests/i2c_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_i2c_idle_state() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("sda_in", 1u8);

    tb.expect("busy", 0u32).await;
    tb.expect("done", 0u32).await;
    tb.expect("scl", 1u32).await;  // SCL high when idle
}

#[tokio::test]
async fn test_i2c_start_transfer() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("sda_in", 1u8);

    tb.expect("busy", 0u32).await;

    // Start a transfer
    tb.set("wr_data", 0xA5u32);
    tb.set("start", 1u8);
    tb.clock(1).await;
    tb.set("start", 0u8);

    tb.expect("busy", 1u32).await;

    // Wait for completion
    for _ in 0..1000 {
        tb.clock(1).await;
        if tb.get_u64("done").await == 1 {
            break;
        }
    }
    tb.expect("done", 1u32).await;
}

#[tokio::test]
async fn test_i2c_returns_to_idle() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("sda_in", 1u8);

    // Start and complete a transfer
    tb.set("wr_data", 0x55u32);
    tb.set("start", 1u8);
    tb.clock(1).await;
    tb.set("start", 0u8);

    for _ in 0..1000 {
        tb.clock(1).await;
        if tb.get_u64("done").await == 1 {
            break;
        }
    }

    // After completion, the FSM should return to idle
    tb.clock(2).await;
    tb.expect("busy", 0u32).await;
    tb.expect("done", 0u32).await;
}
```

**`test_i2c_idle_state`** — after reset, the FSM should be idle: not busy, not done, SCL high.

**`test_i2c_start_transfer`** — pulse `start`, confirm busy, poll for `done`. The `get_u64` loop handles protocol-dependent timing.

**`test_i2c_returns_to_idle`** — after completing a transfer, the FSM must not get stuck. It should return to idle within a few cycles.

---

## Helper Functions

Patterns repeat as your test suite grows. The I2C polling loop appears twice above. Extract it:

```rust
async fn wait_for_done(tb: &mut Testbench, max_cycles: usize) {
    for _ in 0..max_cycles {
        tb.clock(1).await;
        if tb.get_u64("done").await == 1 {
            return;
        }
    }
    panic!("Timeout waiting for done signal after {} cycles", max_cycles);
}
```

Now the I2C test reads cleanly:

```rust
#[tokio::test]
async fn test_i2c_start_transfer_clean() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("sda_in", 1u8);

    tb.set("wr_data", 0xA5u32);
    tb.set("start", 1u8);
    tb.clock(1).await;
    tb.set("start", 0u8);

    tb.expect("busy", 1u32).await;
    wait_for_done(&mut tb, 1000).await;
    tb.expect("done", 1u32).await;
}
```

More useful helpers:

```rust
/// Pulse a signal high for one cycle, then low.
async fn pulse(tb: &mut Testbench, signal: &str) {
    tb.set(signal, 1u8);
    tb.clock(1).await;
    tb.set(signal, 0u8);
}

/// Wait until a signal reaches a specific value, or panic after timeout.
async fn wait_for_value(tb: &mut Testbench, signal: &str, value: u64, max_cycles: usize) {
    for _ in 0..max_cycles {
        if tb.get_u64(signal).await == value {
            return;
        }
        tb.clock(1).await;
    }
    panic!("Timeout: '{}' did not reach {} within {} cycles", signal, value, max_cycles);
}
```

These are regular Rust functions with parameters, return values, and real control flow. This is one of the biggest advantages over VHDL testbenches — you have a real programming language for test infrastructure, not a hardware description language forced into a testing role.

---

## Waveform Debugging

When `expect` fails, the error message usually tells you enough. When it does not — when you need timing relationships between signals or need to trace an FSM through its states — dump a waveform:

```rust
#[tokio::test]
async fn test_i2c_debug() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();
    tb.reset(2).await;
    tb.set("sda_in", 1u8);

    tb.set("wr_data", 0xA5u32);
    tb.set("start", 1u8);
    tb.clock(1).await;
    tb.set("start", 0u8);

    tb.clock(200).await;
    tb.export_waveform("build/i2c_debug.vcd").unwrap();
}
```

Open with `gtkwave build/i2c_debug.vcd`. You will see every signal — inputs, outputs, internals — at every clock edge.

To avoid dumping on every run, gate it behind an environment variable:

```rust
if std::env::var("DUMP_VCD").is_ok() {
    tb.export_waveform("build/counter_test.vcd").unwrap();
}
```

Normal run: `cargo test`. With waveforms: `DUMP_VCD=1 cargo test`.

VCD (Value Change Dump) is the IEEE 1364 standard waveform format. Despite its Verilog origin, it is universally supported — GTKWave, Surfer, Scansion, and the VS Code WaveTrace extension all open VCD files.

---

> **Coming from SystemVerilog/VHDL Testbenches?**
>
> | Traditional Approach | skalp + Rust |
> |---------------------|-------------|
> | Write a VHDL test entity with no ports | Write a Rust function with `#[tokio::test]` |
> | Instantiate the DUT as a component | `Testbench::new("file.vhd")` |
> | Generate a clock with `wait for 10 ns` loops | Built in: `tb.clock(n)` |
> | Drive signals with `<=` and `wait` | `tb.set("signal", value)` |
> | Check outputs with `assert` (often missing) | `tb.expect("signal", value)` fails the test |
> | Run in ModelSim, Questa, Vivado Sim, or GHDL | `cargo test` |
> | License required (ModelSim/Questa) | Free |
> | UVM for reusable test infrastructure | Rust functions, structs, traits |
> | CI requires vendor tools on server | `cargo test` in any CI pipeline |
> | Compile time: seconds to minutes | Compile time: milliseconds |
>
> The most important row is **assertions**. In a traditional VHDL testbench, it is easy to forget an `assert` — the simulation runs, produces a waveform, and you visually inspect it. That is manual verification, not testing. With `expect`, every check is explicit, automated, and fails loudly.
>
> **UVM users:** skalp is not a UVM replacement for constrained random verification. It covers the 90% case — directed tests for specific behaviors. For most designs under 10,000 lines of VHDL, directed tests with helper functions are sufficient and far more maintainable than a UVM environment.

---

## Test Organization

### One File Per Design

Match test files to VHDL source files. Run tests for a single design with `cargo test --test counter_test`.

```
src/                    tests/
  counter.vhd             counter_test.rs
  timer.vhd               timer_test.rs
  i2c_fsm.vhd             i2c_test.rs
  bus_controller.vhd       bus_test.rs
```

### One Test Per Behavior

Name tests after the behavior, not the implementation:

```rust
// Good: reads like a requirement
#[tokio::test] async fn test_counter_wraps_at_255() { ... }
#[tokio::test] async fn test_counter_holds_when_disabled() { ... }
#[tokio::test] async fn test_timer_fires_at_threshold() { ... }

// Bad: implementation detail
#[tokio::test] async fn test_counter_reg_value() { ... }
#[tokio::test] async fn test_state_machine_state_3() { ... }
```

### Shared Helpers

When multiple test files need the same helpers, put them in `tests/common/mod.rs` and import with `mod common; use common::wait_for_signal;`.

### Edge Cases to Always Test

| Category | Example |
|----------|---------|
| **Reset behavior** | Outputs are in a known state after reset |
| **Boundary values** | Counter at 0, counter at max, threshold at 0 |
| **Enable/disable** | Design does nothing when disabled |
| **Overflow/underflow** | Counter wraps, timer fires at exact threshold |
| **Idle return** | FSM returns to idle after completing an operation |
| **Back-to-back** | Start a new operation immediately after the previous one completes |
| **Invalid input** | What happens if `start` is pulsed while busy? |

---

## Quick Reference

| API Method | Signature | Purpose |
|------------|-----------|---------|
| `Testbench::new` | `new(path).await.unwrap()` | Compile VHDL and create simulator |
| `set` | `tb.set("port", value)` | Drive input (takes effect next clock) |
| `clock` | `tb.clock(n).await` | Advance n clock cycles |
| `expect` | `tb.expect("port", value).await` | Assert signal equals value |
| `get_u64` | `tb.get_u64("port").await` | Read signal as u64 |
| `reset` | `tb.reset(n).await` | Assert reset for n cycles, then release |
| `export_waveform` | `tb.export_waveform("f.vcd").unwrap()` | Dump signal history to VCD |

| Task | Command |
|------|---------|
| Run all tests | `cargo test` |
| Run one test file | `cargo test --test counter_test` |
| Run one test by name | `cargo test test_counter_overflow` |
| Run with output visible | `cargo test -- --nocapture` |
| Run with waveform dump | `DUMP_VCD=1 cargo test` |
| Open waveform | `gtkwave build/output.vcd` |

| Rust Syntax | Meaning |
|-------------|---------|
| `#[tokio::test]` | Async test attribute |
| `.await` | Wait for async operation |
| `.unwrap()` | Extract value or panic |
| `1u8`, `0xA5u32` | Typed integer literals |
| `for i in 1..=10u32` | Inclusive range loop |
| `&mut tb` | Mutable reference (for helpers) |

---

## Next: skalp Integration

Your VHDL designs now have a proper test suite that runs with `cargo test` and catches regressions automatically. In Chapter 7, you will learn how skalp-specific pragmas and features enhance your VHDL:

- `-- skalp:` comment pragmas for safety checks, CDC annotations, and signal tracing
- Formal verification with `skalp verify` — prove properties, do not just test them
- Mixed skalp+VHDL designs where some entities use skalp's native language
- Integration with skalp's debug server for VS Code breakpoint debugging

Continue to [Chapter 7: skalp Integration](../07-skalp-integration/).
