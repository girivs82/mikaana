---
title: "Chapter 10: Testing and Verification"
date: 2025-07-15
summary: "Async Rust testbench API -- Testbench::with_top_module(), tb.set(), tb.clock().await, tb.expect().await, tb.get_u64().await. Test organization, helper functions, multiple test cases, waveform generation with VCD, and a complete UART test suite."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 10
ShowToc: true
aliases: ["/tutorial/10-testing/"]
---

## What This Chapter Teaches

You have spent nine chapters building a UART peripheral from scratch: entity declarations, state machines, receivers, FIFOs, parameterization, structs, enums, clock domain crossings, and safety annotations. The design compiles. The compiler has checked your types, your exhaustive matches, your clock domain boundaries, and your safety mechanism coverage. But none of that proves the design does what you intend.

Testing proves intent. The compiler guarantees that your code is well-formed; tests guarantee that your code is correct. In skalp, tests are written in Rust using an async testbench API that drives the simulator. You write ordinary Rust async functions, annotated with `#[tokio::test]`, that set input port values, advance the clock, and assert output port values. The Rust test runner executes them, reports pass/fail, and optionally dumps VCD waveforms for debugging.

By the end of this chapter you will understand:

- How `Testbench::with_top_module("path", "Entity")` creates a simulator instance
- How `tb.set("port", value)` drives input ports and `tb.get_u64("port")` reads signal values
- How `tb.clock(n).await` advances clock cycles
- How `tb.expect("port", value).await` asserts signal values with clear error messages
- How `tb.reset(n).await` asserts and deasserts reset cleanly
- How `tb.export_waveform("file.vcd")` dumps waveforms for GTKWave debugging
- How to write async helper functions that abstract protocol-level operations
- How to organize tests into focused, independent test cases
- How to run tests with `cargo test`

This is the final chapter. By the end, you will have a complete, tested UART peripheral.

---

## Standalone Example: Counter Testbench

Let us start with something familiar. In Chapter 1 you built an 8-bit counter with enable and overflow. Now you will write a proper test suite for it.

Tests live in the `tests/` directory as `.rs` (Rust) files. Create `tests/counter_test.rs`:

```rust
// tests/counter_test.rs
use skalp_testing::Testbench;

#[tokio::test]
async fn test_counter_counts() {
    let mut tb = Testbench::with_top_module("src/counter.sk", "Counter")
        .await.unwrap();
    tb.reset(2).await;

    // Counter should start at 0 after reset
    tb.expect("count", 0u32).await;

    // Enable counting
    tb.set("enable", 1u8);

    // Count up from 1 to 10
    for i in 1..=10u32 {
        tb.clock(1).await;
        tb.expect("count", i).await;
    }
}

#[tokio::test]
async fn test_counter_overflow() {
    let mut tb = Testbench::with_top_module("src/counter.sk", "Counter")
        .await.unwrap();
    tb.reset(2).await;
    tb.set("enable", 1u8);

    // Run to just before overflow (8-bit counter wraps at 256)
    tb.clock(255).await;
    tb.expect("count", 255u32).await;

    // One more cycle -- should wrap to 0 and assert overflow
    tb.clock(1).await;
    tb.expect("count", 0u32).await;
    tb.expect("overflow", 1u32).await;
}

#[tokio::test]
async fn test_counter_disable() {
    let mut tb = Testbench::with_top_module("src/counter.sk", "Counter")
        .await.unwrap();
    tb.reset(2).await;

    // Count up to 5
    tb.set("enable", 1u8);
    tb.clock(5).await;
    tb.expect("count", 5u32).await;

    // Disable -- counter should hold its value
    tb.set("enable", 0u8);
    tb.clock(10).await;
    tb.expect("count", 5u32).await;  // unchanged after 10 cycles

    // Re-enable -- should resume from 5
    tb.set("enable", 1u8);
    tb.clock(1).await;
    tb.expect("count", 6u32).await;
}
```

Run the tests:

```bash
cargo test --test counter_test
```

You should see output like:

```
running 3 tests
test test_counter_counts ... ok
test test_counter_overflow ... ok
test test_counter_disable ... ok

test result: ok. 3 passed; 0 failed; 0 ignored
```

### The Testbench API

The testbench API is async — every operation that interacts with the simulator returns a future that you `.await`. You use `#[tokio::test]` instead of `#[test]` for async test functions.

**Why async?** The skalp simulator compiles your hardware design to a C++ shared library and runs it in a separate thread. The testbench and the simulator communicate through a channel: `tb.clock(n).await` sends a "run N cycles" message to the simulator thread and suspends until it replies with the new state. This architecture keeps the simulator's hot loop free of Rust FFI overhead on every cycle — it runs thousands of C++ evaluation steps in one batch, then reports back. The `async`/`.await` syntax makes this message-passing look sequential. Without it, you would need explicit callbacks or manual thread synchronization. The cost is a `.await` on every call that touches the simulator, but the benefit is that a `tb.clock(10_000).await` runs all 10,000 cycles in native C++ speed without 10,000 FFI round-trips.

Here is the complete reference:

**`Testbench::with_top_module("path.sk", "Entity").await.unwrap()`** -- Creates a new simulator instance. The first argument is the path to the skalp source file, the second is the entity name. The compiler compiles the design to C++, builds it, and loads the resulting shared library for simulation. If the entity has generic parameters, they use their defaults.

**`tb.set("port_name", value)`** -- Sets an input port to a value. The value must have a concrete Rust type (`u8`, `u32`, `u64`) that determines the port width. The new value takes effect at the next clock edge -- calling `tb.set` does not immediately change anything, it stages the value. You can call `tb.set` multiple times before a `tb.clock()` to set up multiple inputs atomically. Note: `tb.set` is **not** async — it returns immediately.

**`tb.get_u64("port_name").await`** -- Returns the current value of any signal as a `u64`. This works on input ports and output ports.

**`tb.clock(n).await`** -- Advances the simulation by `n` clock cycles. Each cycle consists of a rising edge followed by a falling edge. All sequential logic updates on the rising edge. After `tb.clock()` returns, all combinational outputs reflect the new state. Use `tb.clock(1)` for a single cycle or `tb.clock(434)` for many.

**`tb.expect("port_name", value).await`** -- Asserts that the named port currently equals `value`. The value type must match the port width (e.g., `0u32` for a 32-bit check). If the assertion fails, the test panics with a detailed error message including the port name and expected vs actual values.

**`tb.reset(n).await`** -- Asserts the reset signal for `n` clock cycles, then deasserts it. Every test should start with `tb.reset(2).await` to put the design in a known state.

**`tb.export_waveform("filename.vcd").unwrap()`** -- Dumps the complete signal history to a VCD file. Call this at any point during the test. You can open the file in GTKWave or any VCD viewer.

**`tb.get_input_names()` / `tb.get_output_names()`** -- Returns the list of input/output port names. Useful for debugging when you are not sure what signals are available.

### Using `assert_eq!` and `assert!` Directly

The `tb.expect()` method is a convenience wrapper, but you can also use standard Rust assertions for more complex checks:

```rust
// Exact value check with get_u64
let count = tb.get_u64("count").await;
assert_eq!(count, 42, "count should be 42 after 42 cycles");

// Range check
let count = tb.get_u64("count").await;
assert!(count >= 10 && count <= 20, "count {} out of range [10, 20]", count);

// Boolean condition
assert_eq!(tb.get_u64("overflow").await, 0, "overflow should not be asserted yet");
```

Use `tb.expect` for straightforward value checks and Rust assertions for anything more nuanced.

### Helper Functions

As tests grow, you will find yourself repeating sequences of operations: driving a UART byte, capturing a TX frame, reading from a FIFO. Extract these into async helper functions. The key insight is that `Testbench` is a regular Rust struct -- you can pass it by mutable reference to any async function.

```rust
const CYCLES_PER_BIT: usize = 434;

/// Drive a byte onto the RX pin, simulating an external device
/// sending data into our UART receiver.
async fn drive_rx_byte(tb: &mut Testbench, byte: u8) {
    // Start bit (drive low)
    tb.set("rx", 0u8);
    tb.clock(CYCLES_PER_BIT).await;

    // Data bits, LSB first
    for i in 0..8 {
        let bit_val = ((byte >> i) & 1) as u8;
        tb.set("rx", bit_val);
        tb.clock(CYCLES_PER_BIT).await;
    }

    // Stop bit (drive high)
    tb.set("rx", 1u8);
    tb.clock(CYCLES_PER_BIT).await;
}

/// Read a byte from the RX FIFO.
/// rd_data is combinational, so read BEFORE advancing the pointer.
async fn read_rx_byte(tb: &mut Testbench) -> u8 {
    let data = tb.get_u64("rx_data").await as u8;
    tb.set("rx_read", 1u8);
    tb.clock(1).await;
    tb.set("rx_read", 0u8);
    data
}
```

These functions turn low-level port wiggling into protocol-level operations. Your test cases read like specifications: "drive 0xA3 onto RX, read from the FIFO, check it matches." The mechanical details are hidden in the helpers.

---

> **Coming from SystemVerilog?**
>
> SystemVerilog testbenches and skalp testbenches solve the same problem -- driving stimulus and checking results -- but the approach is fundamentally different:
>
> | SystemVerilog | skalp (Rust) | Why it matters |
> |---|---|---|
> | `initial begin ... end` blocks with `#delay` | Async Rust functions with `tb.clock(n).await` | No ambiguous time units; every operation is cycle-accurate |
> | UVM for industrial verification (1000+ lines of boilerplate) | `#[tokio::test]` with `skalp_testing::Testbench` | A complete test case in 20 lines, not 200 |
> | `$display` / `$error` for messages | `assert_eq!`, `assert!`, `tb.expect()` with Rust panic messages | Structured error reporting with file/line info |
> | `$dumpvars` / `$dumpfile` for waveforms | `tb.export_waveform("file.vcd")` | VCD generation without modifying the testbench code |
> | No type safety for port values | Rust type system prevents mixing signal types | Cannot accidentally pass a string where a number is expected |
> | `$random` for randomization | Rust `rand` crate, `proptest` for property-based testing | Full ecosystem of testing libraries |
> | Separate compilation of TB and DUT | `cargo test` handles everything | One command builds, compiles C++ backend, and runs tests |
>
> The biggest shift: UVM is an industrial standard, but it was designed for verification teams with dozens of engineers. A skalp testbench is designed for the hardware engineer who wrote the RTL. You do not need a verification methodology -- you need to check that your counter counts and your FIFO does not overflow. Rust gives you type safety, clear error messages, and access to the entire Rust ecosystem (random number generation, file I/O, data structures) without a custom scripting language.
>
> Tests in skalp are also deterministic by default. Same code, same seed, same results. No race conditions between `initial` blocks, no sensitivity list surprises. If a test passes on your machine, it passes in CI.

---

## Running Project: UART Test Suite

Now let us build a real test suite for the UART peripheral you have been constructing across the tutorial. This is the full UART with transmitter, receiver, and FIFOs — the `UartTop` entity from Chapter 6.

Create `tests/uart_test.rs`:

```rust
// tests/uart_test.rs — Chapter 10: Putting It All Together
//
// Complete test suite for the UART peripheral built across
// Chapters 1-9 of the skalp tutorial.

use skalp_testing::Testbench;

// At 50 MHz clock and 115200 baud, each bit takes ~434 clock cycles.
const CYCLES_PER_BIT: usize = 434;

// A full UART frame: 1 start + 8 data + 1 stop = 10 bits
const CYCLES_PER_FRAME: usize = CYCLES_PER_BIT * 10;

// ----------------------------------------------------------------
// Helper functions
// ----------------------------------------------------------------

/// Drive a byte onto the RX pin, simulating an external device
/// sending data into our UART receiver.
async fn drive_rx_byte(tb: &mut Testbench, byte: u8) {
    // Start bit (drive low)
    tb.set("rx", 0u8);
    tb.clock(CYCLES_PER_BIT).await;

    // Data bits, LSB first
    for i in 0..8 {
        let bit_val = ((byte >> i) & 1) as u8;
        tb.set("rx", bit_val);
        tb.clock(CYCLES_PER_BIT).await;
    }

    // Stop bit (drive high)
    tb.set("rx", 1u8);
    tb.clock(CYCLES_PER_BIT).await;
}

/// Wait for a TX frame to complete and capture the transmitted byte
/// by sampling the TX output pin at mid-bit points.
async fn capture_tx_byte(tb: &mut Testbench) -> u8 {
    // Wait for start bit (TX goes low)
    let mut timeout = CYCLES_PER_FRAME * 2;
    while tb.get_u64("tx").await == 1 && timeout > 0 {
        tb.clock(1).await;
        timeout -= 1;
    }
    assert!(timeout > 0, "Timeout waiting for TX start bit");

    // Advance to middle of start bit
    tb.clock(CYCLES_PER_BIT / 2).await;

    // Verify it is still low (valid start bit)
    assert_eq!(tb.get_u64("tx").await, 0, "Invalid start bit");

    // Sample 8 data bits at mid-bit
    let mut byte: u8 = 0;
    for i in 0..8 {
        tb.clock(CYCLES_PER_BIT).await;
        let bit_val = tb.get_u64("tx").await;
        byte |= (bit_val as u8) << i;
    }

    // Advance to stop bit
    tb.clock(CYCLES_PER_BIT).await;
    assert_eq!(tb.get_u64("tx").await, 1, "Invalid stop bit");

    byte
}

/// Write a byte into the TX FIFO for transmission.
async fn write_tx_byte(tb: &mut Testbench, byte: u8) {
    tb.set("tx_data", byte as u32);
    tb.set("tx_write", 1u8);
    tb.clock(1).await;
    tb.set("tx_write", 0u8);
}

/// Read a byte from the RX FIFO.
/// rd_data = memory[rd_ptr] is combinational, so read BEFORE advancing.
async fn read_rx_byte(tb: &mut Testbench) -> u8 {
    let data = tb.get_u64("rx_data").await as u8;
    // Pulse rx_read to advance FIFO read pointer
    tb.set("rx_read", 1u8);
    tb.clock(1).await;
    tb.set("rx_read", 0u8);
    data
}

// ----------------------------------------------------------------
// Test cases: Transmitter
// ----------------------------------------------------------------

#[tokio::test]
async fn test_uart_tx_basic() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // Write 0x55 (alternating bits) into TX FIFO
    write_tx_byte(&mut tb, 0x55).await;

    // Capture the transmitted byte
    let captured = capture_tx_byte(&mut tb).await;
    assert_eq!(captured, 0x55, "TX mismatch: expected 0x55, got 0x{:02X}", captured);
}

#[tokio::test]
async fn test_uart_tx_consecutive_bytes() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // Transmit two bytes back-to-back
    let byte1: u8 = 0xAA;
    let byte2: u8 = 0x55;

    write_tx_byte(&mut tb, byte1).await;
    let captured1 = capture_tx_byte(&mut tb).await;
    assert_eq!(captured1, byte1, "First byte mismatch");

    write_tx_byte(&mut tb, byte2).await;
    let captured2 = capture_tx_byte(&mut tb).await;
    assert_eq!(captured2, byte2, "Second byte mismatch");
}

// ----------------------------------------------------------------
// Test cases: Receiver
// ----------------------------------------------------------------

#[tokio::test]
async fn test_uart_rx_basic() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);
    tb.clock(10).await;

    // Drive 0xA3 onto the RX pin
    drive_rx_byte(&mut tb, 0xA3).await;
    tb.clock(5).await;

    // Read from the RX FIFO
    let received = read_rx_byte(&mut tb).await;
    assert_eq!(received, 0xA3, "RX data mismatch");
}

#[tokio::test]
async fn test_uart_rx_all_zeros() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);
    tb.clock(10).await;

    drive_rx_byte(&mut tb, 0x00).await;
    tb.clock(5).await;

    let received = read_rx_byte(&mut tb).await;
    assert_eq!(received, 0x00, "All-zeros byte should be received correctly");
}

#[tokio::test]
async fn test_uart_rx_all_ones() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);
    tb.clock(10).await;

    drive_rx_byte(&mut tb, 0xFF).await;
    tb.clock(5).await;

    let received = read_rx_byte(&mut tb).await;
    assert_eq!(received, 0xFF, "All-ones byte should be received correctly");
}

#[tokio::test]
async fn test_uart_rx_multiple_bytes() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);
    tb.clock(10).await;

    let test_data: [u8; 5] = [0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"

    // Drive all bytes onto the RX pin
    for &byte in &test_data {
        drive_rx_byte(&mut tb, byte).await;
    }

    // Read them back from the FIFO and verify order
    for (i, &expected) in test_data.iter().enumerate() {
        let received = read_rx_byte(&mut tb).await;
        assert_eq!(
            received, expected,
            "Byte {} mismatch: expected 0x{:02X}, got 0x{:02X}",
            i, expected, received
        );
    }
}

// ----------------------------------------------------------------
// Test cases: Loopback
// ----------------------------------------------------------------

#[tokio::test]
async fn test_uart_loopback() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // Send a byte via TX
    let test_byte: u8 = 0x42;
    write_tx_byte(&mut tb, test_byte).await;

    // In a loopback test, connect the TX output to the RX input
    // on every cycle. This simulates a physical loopback connection.
    for _ in 0..(CYCLES_PER_FRAME + 100) {
        let tx_val = tb.get_u64("tx").await as u8;
        tb.set("rx", tx_val);
        tb.clock(1).await;
    }

    // The byte should have traveled: TX FIFO -> TX FSM -> tx pin
    // -> rx pin -> RX FSM -> RX FIFO
    let received = read_rx_byte(&mut tb).await;
    assert_eq!(
        received, test_byte,
        "Loopback failed: sent 0x{:02X}, got 0x{:02X}",
        test_byte, received
    );
}

#[tokio::test]
async fn test_uart_loopback_multiple() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    let test_data: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

    for &byte in &test_data {
        write_tx_byte(&mut tb, byte).await;

        // Loopback for one full frame
        for _ in 0..(CYCLES_PER_FRAME + 100) {
            let tx_val = tb.get_u64("tx").await as u8;
            tb.set("rx", tx_val);
            tb.clock(1).await;
        }

        let received = read_rx_byte(&mut tb).await;
        assert_eq!(
            received, byte,
            "Loopback mismatch: sent 0x{:02X}, got 0x{:02X}",
            byte, received
        );
    }
}

// ----------------------------------------------------------------
// Test cases: FIFO behavior
// ----------------------------------------------------------------

#[tokio::test]
async fn test_uart_fifo_buffering() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // Write 5 bytes to the TX FIFO before any are transmitted.
    let message: [u8; 5] = [0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"
    for &byte in &message {
        write_tx_byte(&mut tb, byte).await;
    }

    // FIFO should not be full (depth 16, wrote 5)
    tb.expect("tx_fifo_full", 0u32).await;

    // Let all bytes transmit
    tb.clock(CYCLES_PER_FRAME * 5 + 500).await;

    // TX FIFO should be empty now
    tb.expect("tx_fifo_empty", 1u32).await;
}

// ----------------------------------------------------------------
// Test cases: Edge cases and error conditions
// ----------------------------------------------------------------

#[tokio::test]
async fn test_uart_idle_state() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // After reset, TX should be idle (high) and FIFOs should be empty
    tb.expect("tx", 1u32).await;
    tb.expect("tx_fifo_full", 0u32).await;
    tb.expect("rx_fifo_empty", 1u32).await;

    // Run for a while with no activity -- nothing should change
    tb.clock(1000).await;
    tb.expect("tx", 1u32).await;
}

#[tokio::test]
async fn test_uart_reset_clears_state() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // Write some data to create internal state
    write_tx_byte(&mut tb, 0xAA).await;
    tb.clock(100).await;

    // Drive a byte into RX
    drive_rx_byte(&mut tb, 0x55).await;
    tb.clock(5).await;

    // Assert reset
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // After reset, everything should be clean
    tb.expect("tx", 1u32).await;
    tb.expect("tx_fifo_full", 0u32).await;
    tb.expect("tx_fifo_empty", 1u32).await;
    tb.expect("rx_fifo_empty", 1u32).await;
}

// ----------------------------------------------------------------
// Test cases: Waveform capture
// ----------------------------------------------------------------

#[tokio::test]
async fn test_uart_with_waveform_dump() {
    let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);

    // Send a recognizable pattern
    write_tx_byte(&mut tb, 0x55).await;
    tb.clock(CYCLES_PER_FRAME + 100).await;

    drive_rx_byte(&mut tb, 0xAA).await;
    tb.clock(5).await;

    // Save waveform for manual inspection
    tb.export_waveform("uart_debug.vcd").unwrap();

    let received = read_rx_byte(&mut tb).await;
    assert_eq!(received, 0xAA);
}
```

### Test Organization

The test file above is organized into sections by functionality: transmitter tests, receiver tests, loopback tests, FIFO tests, and edge cases. Each test function is independent -- it creates its own `Testbench`, resets the design, and runs to completion without depending on any other test. This matters because the Rust test runner can execute tests in parallel and in any order.

Guidelines for organizing tests:

**One assertion theme per test.** `test_uart_tx_basic` checks that the transmitter sends the right bits. `test_uart_fifo_buffering` checks that the FIFO absorbs and drains. Do not combine unrelated checks into a single test -- when it fails, you want the test name to tell you what broke.

**Name tests descriptively.** `test_uart_rx_all_zeros` is better than `test_rx_2`. When a CI run shows a failure, the name should tell you where to look.

**Use helper functions for protocol operations.** The `drive_rx_byte`, `capture_tx_byte`, `write_tx_byte`, and `read_rx_byte` helpers keep each test case focused on intent rather than bit-level mechanics.

**Include edge cases.** The idle state test and reset test exercise conditions that are easy to miss in a design review but critical for correctness. Real hardware will encounter all of these.

**Keep timeouts finite.** The `capture_tx_byte` helper has a maximum cycle count for waiting on the start bit. Without it, a bug could cause a test to spin forever. In CI, a hung test wastes build minutes and gives no useful feedback.

---

## Build and Test

Your final project structure should look like this:

```
uart-tutorial/
  Cargo.toml
  src/
    counter.sk
    uart_tx.sk
    uart_rx.sk
    fifo.sk
    uart_buffered.sk
    uart_loopback.sk
    uart_top.sk
  tests/
    counter_test.rs      <- Chapter 10 (standalone example)
    uart_test.rs         <- Chapter 10 (running project)
```

Run the entire test suite:

```bash
cargo test
```

Expected output:

```
running 3 tests
test test_counter_counts ... ok
test test_counter_overflow ... ok
test test_counter_disable ... ok

test result: ok. 3 passed; 0 failed; 0 ignored

running 12 tests
test test_uart_tx_basic ... ok
test test_uart_tx_consecutive_bytes ... ok
test test_uart_rx_basic ... ok
test test_uart_rx_all_zeros ... ok
test test_uart_rx_all_ones ... ok
test test_uart_rx_multiple_bytes ... ok
test test_uart_loopback ... ok
test test_uart_loopback_multiple ... ok
test test_uart_fifo_buffering ... ok
test test_uart_idle_state ... ok
test test_uart_reset_clears_state ... ok
test test_uart_with_waveform_dump ... ok

test result: ok. 12 passed; 0 failed; 0 ignored
```

Run a single test by name:

```bash
cargo test test_uart_loopback
```

Run all tests matching a pattern:

```bash
cargo test test_uart_rx     # run all tests matching "rx"
```

Show `println!` output during tests:

```bash
cargo test -- --nocapture
```

Open a VCD waveform file generated by `export_waveform()` in GTKWave:

```bash
gtkwave uart_debug.vcd
```

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Create testbench | `Testbench::with_top_module("path", "Entity").await.unwrap()` | `let mut tb = Testbench::with_top_module("src/uart_top.sk", "UartTop").await.unwrap();` |
| Set input port | `tb.set("port", value)` | `tb.set("rx", 1u8);` |
| Read signal value | `tb.get_u64("port").await` | `let v = tb.get_u64("tx").await;` |
| Assert port value | `tb.expect("port", value).await` | `tb.expect("count", 42u32).await;` |
| Advance N cycles | `tb.clock(n).await` | `tb.clock(434).await;` |
| Assert then deassert reset | `tb.reset(n).await` | `tb.reset(2).await;` |
| Save waveform | `tb.export_waveform("file.vcd").unwrap()` | `tb.export_waveform("debug.vcd").unwrap();` |
| Test attribute | `#[tokio::test]` | `#[tokio::test] async fn test_foo() { ... }` |
| Import testbench | `use skalp_testing::Testbench;` | Top of every test file |
| Run all tests | `cargo test` | From project root |
| Run single test | `cargo test <name>` | `cargo test test_uart_loopback` |
| Run with output | `cargo test -- --nocapture` | Shows `println!` output |

---

## Tutorial Complete

Over ten chapters, you have built a complete UART peripheral from scratch in skalp:

**Chapter 1** introduced entities, ports, signals, and the `on(clk.rise)` / combinational split. You built a counter.

**Chapter 2** added state machines. You built the UART transmitter with baud rate timing, a default-decrement pattern, and shift register serialization.

**Chapter 3** built the UART receiver with mid-bit sampling and match-based state machine transitions.

**Chapter 4** introduced arrays and generics. You built a parameterized FIFO with combinational reads and modulo pointer wrapping, and added buffering to the UART.

**Chapter 5** covered const generics and parameterization. The UART became configurable -- baud rate, FIFO depth, data width.

**Chapter 6** introduced composition at scale. You built UartTop with transmitter, receiver, and FIFOs composed via `let` bindings and dot-notation output access.

**Chapter 7** added enums and pattern matching. FSM states became typed, match expressions became exhaustive, and the command parser decoded incoming bytes into safe enum values.

**Chapter 8** tackled clock domain crossing. Clock lifetimes made the compiler enforce CDC safety, and an async FIFO connected different clock domains.

**Chapter 9** added safety annotations: TMR voting for radiation hardening, `#[trace]` for debug visibility, and `#[breakpoint]` for halt-on-condition debugging.

**Chapter 10** -- this chapter -- built a complete async Rust test suite that exercises every feature of the design.

The final design is a UART peripheral with:

- Transmitter and receiver with configurable baud rate
- 16-deep TX and RX FIFOs with full/empty flags
- State machines with exhaustive match transitions
- Hierarchical composition via `let` bindings and dot-notation
- Enum-driven state machines with type safety
- A Rust test suite with 12 async tests covering transmitter, receiver, loopback, FIFO buffering, and edge cases

This is the kind of IP block that forms the backbone of real hardware projects -- an interface controller that sits between a processor bus and the physical world.

### Where to Go Next

The tutorial covered the skalp language and workflow. For deeper topics, see:

- **[skalp project page](/projects/skalp/)** -- Compiler architecture, intermediate representations, and the lowering pipeline from skalp source to synthesizable SystemVerilog.
- **[Null Convention Logic](/blog/null-convention-logic/)** -- How skalp supports asynchronous circuit design with NCL, removing the global clock entirely.
- **[GitHub repository](https://github.com/girivs82/skalp)** -- Source code, more examples, and issue tracker.

Everything you have learned here scales to larger designs.
