---
title: "Chapter 10: Testing and Verification"
date: 2025-07-15
summary: "Rust testbench API in depth -- Testbench::new(), tb.set(), tb.clock(), tb.expect(), tb.get(). Test organization, helper functions, multiple test cases, waveform generation with VCD, and a complete UART test suite."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 10
ShowToc: true
---

## What This Chapter Teaches

You have spent nine chapters building a UART peripheral from scratch: entity declarations, state machines, receivers, FIFOs, parameterization, structs, enums, clock domain crossings, and safety annotations. The design compiles. The compiler has checked your types, your exhaustive matches, your clock domain boundaries, and your safety mechanism coverage. But none of that proves the design does what you intend.

Testing proves intent. The compiler guarantees that your code is well-formed; tests guarantee that your code is correct. In skalp, tests are written in Rust using a testbench API that drives the simulator. You write ordinary Rust functions, annotated with `#[test]`, that set input port values, advance the clock, and assert output port values. The Rust test runner executes them, reports pass/fail, and optionally dumps VCD waveforms for debugging.

By the end of this chapter you will understand:

- How `Testbench::new("EntityName")` creates a simulator instance of any entity
- How `tb.set("port", value)` drives input ports and `tb.get("port")` reads signal values
- How `tb.clock()` advances one clock cycle and `tb.run(n)` advances many
- How `tb.expect("port", value)` asserts signal values with clear error messages
- How `tb.reset(n)` asserts and deasserts reset cleanly
- How `tb.save_vcd("file.vcd")` dumps waveforms for GTKWave debugging
- How to write helper functions that abstract protocol-level operations
- How to organize tests into focused, independent test cases
- How to run tests with `skalp test` and generate waveforms with `skalp test --vcd`

This is the final chapter. By the end, you will have a complete, tested UART peripheral.

---

## Standalone Example: Counter Testbench

Let us start with something familiar. In Chapter 1 you built an 8-bit counter with enable and overflow. Now you will write a proper test suite for it.

Tests live in the `tests/` directory as `.rs` (Rust) files. Create `tests/counter_test.rs`:

```rust
// tests/counter_test.rs
use skalp_test::Testbench;

#[test]
fn test_counter_counts() {
    let mut tb = Testbench::new("Counter");
    tb.reset(2);

    // Counter should start at 0 after reset
    tb.expect("count", 0);

    // Enable counting
    tb.set("enable", 1);

    // Count up from 1 to 10
    for i in 1..=10 {
        tb.clock();
        tb.expect("count", i);
    }
}

#[test]
fn test_counter_overflow() {
    let mut tb = Testbench::new("Counter");
    tb.reset(2);
    tb.set("enable", 1);

    // Run to just before overflow (8-bit counter wraps at 256)
    tb.run(255);
    tb.expect("count", 255);

    // One more cycle -- should wrap to 0 and assert overflow
    tb.clock();
    tb.expect("count", 0);
    tb.expect("overflow", 1);
}

#[test]
fn test_counter_disable() {
    let mut tb = Testbench::new("Counter");
    tb.reset(2);

    // Count up to 5
    tb.set("enable", 1);
    tb.run(5);
    tb.expect("count", 5);

    // Disable -- counter should hold its value
    tb.set("enable", 0);
    tb.run(10);
    tb.expect("count", 5);  // unchanged after 10 cycles

    // Re-enable -- should resume from 5
    tb.set("enable", 1);
    tb.clock();
    tb.expect("count", 6);
}
```

Run the tests:

```bash
skalp test
```

You should see output like:

```
   Compiling uart-tutorial v0.1.0
   Simulating Counter (3 tests)

running 3 tests
test test_counter_counts ... ok
test test_counter_overflow ... ok
test test_counter_disable ... ok

test result: ok. 3 passed; 0 failed; 0 ignored
```

For verbose output with waveform generation:

```bash
skalp test --vcd
```

This saves VCD files for every test to `build/waveforms/`, named after the test function. You can open them in GTKWave to inspect exactly what happened.

### The Testbench API

Every function in the API maps to a specific simulator operation. Here is the complete reference:

**`Testbench::new("EntityName")`** -- Creates a new simulator instance of the named entity. The entity is loaded from your skalp project. If the entity has generic parameters, they are resolved from `skalp.toml` or use their defaults. All input ports start at zero, all output ports are undefined until the first clock edge.

**`tb.set("port_name", value)`** -- Sets an input port to a value. The value is a `u64` that gets truncated to the port width. The new value takes effect at the next clock edge -- calling `tb.set` does not immediately change anything, it stages the value. You can call `tb.set` multiple times before a `tb.clock()` to set up multiple inputs atomically.

**`tb.get("port_name")`** -- Returns the current value of any signal as a `u64`. This works on input ports, output ports, and internal signals that are annotated with `#[trace]` (from Chapter 9). For signals wider than 64 bits, only the lower 64 bits are returned.

**`tb.clock()`** -- Advances the simulation by one clock cycle: a rising edge followed by a falling edge. All sequential logic updates on the rising edge. After `tb.clock()` returns, all combinational outputs reflect the new state. This is the fundamental time-advancement operation.

**`tb.run(n)`** -- Advances `n` clock cycles. Equivalent to calling `tb.clock()` in a loop `n` times, but faster because the simulator can batch them. Use `tb.run` when you need to wait for something but do not need to inspect intermediate states.

**`tb.expect("port_name", value)`** -- Asserts that the named port currently equals `value`. If it does not, the test panics with a detailed error message:

```
assertion failed: port "count" expected 5, got 4
  at tests/counter_test.rs:23
  simulation time: 12 cycles after reset
```

This is more informative than a bare `assert_eq!` because it includes the port name and simulation time. Use `tb.expect` instead of manual `assert_eq!(tb.get("count"), 5)` whenever possible.

**`tb.reset(n)`** -- Asserts the reset signal for `n` clock cycles, then deasserts it. This is equivalent to:

```rust
tb.set("rst", 1);
tb.run(n);
tb.set("rst", 0);
```

Every test should start with `tb.reset()` to put the design in a known state.

**`tb.save_vcd("filename.vcd")`** -- Dumps the complete signal history to a VCD file. Call this at any point during the test -- it writes everything from the start of the test up to the current cycle. You can call it multiple times to capture snapshots. The file is written relative to `build/waveforms/`.

### Using `assert_eq!` and `assert!` Directly

The `tb.expect()` method is a convenience wrapper, but you can also use standard Rust assertions for more complex checks:

```rust
// Exact value check (prefer tb.expect for simple cases)
assert_eq!(tb.get("count"), 42, "count should be 42 after 42 cycles");

// Range check
let count = tb.get("count");
assert!(count >= 10 && count <= 20, "count {} out of range [10, 20]", count);

// Boolean condition
assert!(tb.get("overflow") == 0, "overflow should not be asserted yet");
```

Use `tb.expect` for straightforward value checks and Rust assertions for anything more nuanced.

### Helper Functions

As tests grow, you will find yourself repeating sequences of operations: sending a byte, waiting for a flag, checking a status register. Extract these into helper functions. The key insight is that `Testbench` is a regular Rust struct -- you can pass it by mutable reference to any function.

```rust
/// Drive a complete byte onto the UART RX pin, bit by bit.
/// Start bit (low), 8 data bits LSB-first, stop bit (high).
fn send_byte(tb: &mut Testbench, byte: u8) {
    tb.set("tx_data", byte as u64);
    tb.set("tx_start", 1);
    tb.clock();
    tb.set("tx_start", 0);

    // Wait for transmission to complete
    while tb.get("tx_busy") == 1 {
        tb.clock();
    }
}

/// Wait for the receiver to produce a valid byte, then read it.
fn receive_byte(tb: &mut Testbench) -> u8 {
    while tb.get("rx_valid") == 0 {
        tb.clock();
    }
    tb.get("rx_data") as u8
}
```

These functions turn low-level port wiggling into protocol-level operations. Your test cases read like specifications: "send 0x55, receive a byte, check it matches." The mechanical details are hidden in the helpers.

Add a timeout to avoid infinite loops if something goes wrong:

```rust
fn receive_byte_timeout(tb: &mut Testbench, max_cycles: u64) -> Option<u8> {
    for _ in 0..max_cycles {
        if tb.get("rx_valid") == 1 {
            return Some(tb.get("rx_data") as u8);
        }
        tb.clock();
    }
    None  // timed out
}
```

---

> **Coming from SystemVerilog?**
>
> SystemVerilog testbenches and skalp testbenches solve the same problem -- driving stimulus and checking results -- but the approach is fundamentally different:
>
> | SystemVerilog | skalp (Rust) | Why it matters |
> |---|---|---|
> | `initial begin ... end` blocks with `#delay` | Rust functions with `tb.clock()` / `tb.run()` | No ambiguous time units; every operation is cycle-accurate |
> | UVM for industrial verification (1000+ lines of boilerplate) | Standard Rust `#[test]` with `skalp_test::Testbench` | A complete test case in 20 lines, not 200 |
> | `$display` / `$error` for messages | `assert_eq!`, `assert!`, `tb.expect()` with Rust panic messages | Structured error reporting with file/line info |
> | `$dumpvars` / `$dumpfile` for waveforms | `tb.save_vcd("file.vcd")` or `skalp test --vcd` | VCD generation without modifying the testbench code |
> | No type safety for port values | Rust type system prevents mixing signal types | Cannot accidentally pass a string where a number is expected |
> | `$random` for randomization | Rust `rand` crate, `proptest` for property-based testing | Full ecosystem of testing libraries |
> | Separate compilation of TB and DUT | `skalp test` handles everything | One command builds, elaborates, simulates, and reports |
>
> The biggest shift: UVM is an industrial standard, but it was designed for verification teams with dozens of engineers. A skalp testbench is designed for the hardware engineer who wrote the RTL. You do not need a verification methodology -- you need to check that your counter counts and your FIFO does not overflow. Rust gives you type safety, clear error messages, and access to the entire Rust ecosystem (random number generation, file I/O, data structures) without a custom scripting language.
>
> Tests in skalp are also deterministic by default. Same code, same seed, same results. No race conditions between `initial` blocks, no sensitivity list surprises. If a test passes on your machine, it passes in CI.

---

## Running Project: UART Test Suite

Now let us build a real test suite for the UART peripheral you have been constructing across all ten chapters. This is the full UART with transmitter, receiver, FIFOs, enum-driven state machines, struct-based configuration, clock domain crossing, and safety annotations.

Create `tests/uart_test.rs`:

```rust
// tests/uart_test.rs
//
// Complete test suite for the UART peripheral built across
// Chapters 1-9 of the skalp tutorial.

use skalp_test::Testbench;

// At 50 MHz clock and 115200 baud, each bit takes ~434 clock cycles.
// This matches the default CYCLES_PER_BIT parameter in skalp.toml.
const CYCLES_PER_BIT: u64 = 434;

// A full UART frame: 1 start + 8 data + 1 stop = 10 bits
const CYCLES_PER_FRAME: u64 = CYCLES_PER_BIT * 10;

// ----------------------------------------------------------------
// Helper functions
// ----------------------------------------------------------------

/// Drive a byte onto the RX pin, simulating an external device
/// sending data into our UART receiver.
///
/// Protocol: idle (high) -> start bit (low) -> 8 data bits (LSB first) -> stop bit (high)
fn drive_rx_byte(tb: &mut Testbench, byte: u8) {
    // Start bit (drive low)
    tb.set("rx", 0);
    tb.run(CYCLES_PER_BIT);

    // Data bits, LSB first
    for i in 0..8 {
        let bit_val = (byte >> i) & 1;
        tb.set("rx", bit_val as u64);
        tb.run(CYCLES_PER_BIT);
    }

    // Stop bit (drive high)
    tb.set("rx", 1);
    tb.run(CYCLES_PER_BIT);
}

/// Wait for a TX frame to complete and capture the transmitted byte
/// by sampling the TX output pin at mid-bit points.
fn capture_tx_byte(tb: &mut Testbench) -> u8 {
    // Wait for start bit (TX goes low)
    let mut timeout = CYCLES_PER_FRAME * 2;
    while tb.get("tx") == 1 && timeout > 0 {
        tb.clock();
        timeout -= 1;
    }
    assert!(timeout > 0, "Timeout waiting for TX start bit");

    // Advance to middle of start bit
    tb.run(CYCLES_PER_BIT / 2);

    // Verify it is still low (valid start bit)
    assert_eq!(tb.get("tx"), 0, "Invalid start bit");

    // Sample 8 data bits at mid-bit
    let mut byte: u8 = 0;
    for i in 0..8 {
        tb.run(CYCLES_PER_BIT);
        let bit_val = tb.get("tx");
        byte |= (bit_val as u8) << i;
    }

    // Advance to stop bit
    tb.run(CYCLES_PER_BIT);
    assert_eq!(tb.get("tx"), 1, "Invalid stop bit");

    byte
}

/// Write a byte into the TX FIFO for transmission.
fn write_tx_byte(tb: &mut Testbench, byte: u8) {
    tb.set("tx_data", byte as u64);
    tb.set("tx_write", 1);
    tb.clock();
    tb.set("tx_write", 0);
}

/// Read a byte from the RX FIFO.
fn read_rx_byte(tb: &mut Testbench) -> u8 {
    tb.set("rx_read", 1);
    tb.clock();
    let data = tb.get("rx_data") as u8;
    tb.set("rx_read", 0);
    data
}

/// Wait for a specific signal to become the expected value,
/// with a cycle timeout to prevent infinite loops.
fn wait_for(tb: &mut Testbench, signal: &str, value: u64, max_cycles: u64) {
    for _ in 0..max_cycles {
        if tb.get(signal) == value {
            return;
        }
        tb.clock();
    }
    panic!(
        "Timeout after {} cycles waiting for {} == {}",
        max_cycles, signal, value
    );
}

// ----------------------------------------------------------------
// Test cases: Transmitter
// ----------------------------------------------------------------

#[test]
fn test_uart_tx_basic() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1); // RX idle high

    // Write 0x55 (alternating bits: 01010101) into TX FIFO
    write_tx_byte(&mut tb, 0x55);

    // Wait one cycle for the TX FSM to latch
    tb.run(1);

    // Verify start bit: TX should go low
    assert_eq!(tb.get("tx"), 0, "Expected start bit (low)");

    // Verify each data bit: 0x55 = 01010101, sent LSB first
    for i in 0..8 {
        tb.run(CYCLES_PER_BIT);
        let expected = (0x55 >> i) & 1;
        assert_eq!(
            tb.get("tx"),
            expected,
            "Data bit {} mismatch: expected {}, got {}",
            i,
            expected,
            tb.get("tx")
        );
    }

    // Verify stop bit: TX should return high
    tb.run(CYCLES_PER_BIT);
    assert_eq!(tb.get("tx"), 1, "Expected stop bit (high)");
}

#[test]
fn test_uart_tx_consecutive_bytes() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    // Transmit two bytes back-to-back
    let byte1: u8 = 0xAA;
    let byte2: u8 = 0x55;

    write_tx_byte(&mut tb, byte1);
    let captured1 = capture_tx_byte(&mut tb);
    assert_eq!(captured1, byte1, "First byte mismatch");

    write_tx_byte(&mut tb, byte2);
    let captured2 = capture_tx_byte(&mut tb);
    assert_eq!(captured2, byte2, "Second byte mismatch");
}

// ----------------------------------------------------------------
// Test cases: Receiver
// ----------------------------------------------------------------

#[test]
fn test_uart_rx_basic() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1); // idle
    tb.run(10);       // settle

    // Drive 0xA3 onto the RX pin
    drive_rx_byte(&mut tb, 0xA3);

    // Small settling time for the RX FSM
    tb.run(5);

    // Read from the RX FIFO
    let received = read_rx_byte(&mut tb);
    assert_eq!(received, 0xA3, "RX data mismatch");
}

#[test]
fn test_uart_rx_all_zeros() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);
    tb.run(10);

    drive_rx_byte(&mut tb, 0x00);
    tb.run(5);

    let received = read_rx_byte(&mut tb);
    assert_eq!(received, 0x00, "All-zeros byte should be received correctly");
}

#[test]
fn test_uart_rx_all_ones() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);
    tb.run(10);

    drive_rx_byte(&mut tb, 0xFF);
    tb.run(5);

    let received = read_rx_byte(&mut tb);
    assert_eq!(received, 0xFF, "All-ones byte should be received correctly");
}

#[test]
fn test_uart_rx_multiple_bytes() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);
    tb.run(10);

    let test_data: [u8; 5] = [0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"

    // Drive all bytes onto the RX pin
    for &byte in &test_data {
        drive_rx_byte(&mut tb, byte);
    }

    // Read them back from the FIFO and verify order
    for (i, &expected) in test_data.iter().enumerate() {
        let received = read_rx_byte(&mut tb);
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

#[test]
fn test_uart_loopback() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    // Send a byte via TX
    let test_byte: u8 = 0x42;
    write_tx_byte(&mut tb, test_byte);

    // In a loopback test, connect the TX output to the RX input
    // on every cycle. This simulates a physical loopback connection.
    for _ in 0..(CYCLES_PER_FRAME + 100) {
        let tx_val = tb.get("tx");
        tb.set("rx", tx_val);
        tb.clock();
    }

    // The byte should have traveled: TX FIFO -> TX FSM -> tx pin
    // -> rx pin -> RX FSM -> RX FIFO
    let received = read_rx_byte(&mut tb);
    assert_eq!(
        received, test_byte,
        "Loopback failed: sent 0x{:02X}, got 0x{:02X}",
        test_byte, received
    );
}

#[test]
fn test_uart_loopback_multiple() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    let test_data: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];

    for &byte in &test_data {
        write_tx_byte(&mut tb, byte);

        // Loopback for one full frame
        for _ in 0..(CYCLES_PER_FRAME + 100) {
            let tx_val = tb.get("tx");
            tb.set("rx", tx_val);
            tb.clock();
        }

        let received = read_rx_byte(&mut tb);
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

#[test]
fn test_uart_fifo_buffering() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    // Write 5 bytes to the TX FIFO before any are transmitted.
    // The FIFO (depth 16) should absorb them all.
    let message: [u8; 5] = [0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"
    for &byte in &message {
        write_tx_byte(&mut tb, byte);
    }

    // FIFO should not be full (depth 16, wrote 5)
    assert_eq!(tb.get("status_tx_fifo_full"), 0, "TX FIFO should not be full");

    // Let all bytes transmit. Each frame is ~10 bits * 434 cycles.
    // 5 frames plus margin.
    tb.run(CYCLES_PER_FRAME * 5 + 500);

    // TX FIFO should be empty now
    assert_eq!(tb.get("status_tx_fifo_empty"), 1, "TX FIFO should be empty");
}

#[test]
fn test_uart_rx_fifo_overflow() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);
    tb.run(10);

    // Fill the RX FIFO (depth 16) without reading any bytes
    for i in 0..16 {
        drive_rx_byte(&mut tb, i as u8);
    }
    tb.run(5); // settle

    // FIFO should now be full
    assert_eq!(
        tb.get("status_rx_fifo_full"),
        1,
        "RX FIFO should be full after 16 bytes"
    );

    // Send one more byte -- this should trigger an overrun error
    drive_rx_byte(&mut tb, 0xFF);
    tb.run(5);

    assert_eq!(
        tb.get("status_overrun_error"),
        1,
        "Overrun error should be asserted"
    );
}

// ----------------------------------------------------------------
// Test cases: Edge cases and error conditions
// ----------------------------------------------------------------

#[test]
fn test_uart_idle_state() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    // After reset, TX should be idle (high) and no errors
    tb.expect("tx", 1);
    assert_eq!(tb.get("status_overrun_error"), 0);
    assert_eq!(tb.get("status_tx_fifo_full"), 0);
    assert_eq!(tb.get("status_rx_fifo_empty"), 1);

    // Run for a while with no activity -- nothing should change
    tb.run(1000);
    tb.expect("tx", 1);
    assert_eq!(tb.get("status_overrun_error"), 0);
}

#[test]
fn test_uart_reset_clears_state() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    // Write some data to create internal state
    write_tx_byte(&mut tb, 0xAA);
    tb.run(100);

    // Drive a byte into RX
    drive_rx_byte(&mut tb, 0x55);
    tb.run(5);

    // Assert reset
    tb.reset(2);

    // After reset, everything should be clean
    tb.expect("tx", 1);
    assert_eq!(tb.get("status_tx_fifo_full"), 0);
    assert_eq!(tb.get("status_tx_fifo_empty"), 1);
    assert_eq!(tb.get("status_rx_fifo_empty"), 1);
    assert_eq!(tb.get("status_overrun_error"), 0);
}

#[test]
fn test_uart_frame_error() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);
    tb.run(10);

    // Send a byte but corrupt the stop bit.
    // Start bit:
    tb.set("rx", 0);
    tb.run(CYCLES_PER_BIT);

    // 8 data bits (value does not matter):
    for _ in 0..8 {
        tb.set("rx", 0);
        tb.run(CYCLES_PER_BIT);
    }

    // Stop bit should be high, but we drive it low (frame error)
    tb.set("rx", 0);
    tb.run(CYCLES_PER_BIT);

    // Return to idle
    tb.set("rx", 1);
    tb.run(10);

    // The receiver should report a framing error
    assert_eq!(
        tb.get("status_frame_error"),
        1,
        "Frame error should be asserted for invalid stop bit"
    );
}

// ----------------------------------------------------------------
// Test cases: Waveform capture
// ----------------------------------------------------------------

#[test]
fn test_uart_with_vcd_dump() {
    let mut tb = Testbench::new("UartTop");
    tb.reset(2);
    tb.set("rx", 1);

    // Send a recognizable pattern
    write_tx_byte(&mut tb, 0x55);
    tb.run(CYCLES_PER_FRAME + 100);

    drive_rx_byte(&mut tb, 0xAA);
    tb.run(5);

    // Save waveform for manual inspection in GTKWave
    tb.save_vcd("uart_debug.vcd");

    let received = read_rx_byte(&mut tb);
    assert_eq!(received, 0xAA);
}
```

### Test Organization

The test file above is organized into sections by functionality: transmitter tests, receiver tests, loopback tests, FIFO tests, and edge cases. Each test function is independent -- it creates its own `Testbench`, resets the design, and runs to completion without depending on any other test. This matters because the Rust test runner can execute tests in parallel and in any order.

Guidelines for organizing tests:

**One assertion theme per test.** `test_uart_tx_basic` checks that the transmitter sends the right bits. `test_uart_fifo_buffering` checks that the FIFO absorbs and drains. Do not combine unrelated checks into a single test -- when it fails, you want the test name to tell you what broke.

**Name tests descriptively.** `test_uart_rx_all_zeros` is better than `test_rx_2`. When a CI run shows a failure, the name should tell you where to look.

**Use helper functions for protocol operations.** The `drive_rx_byte`, `capture_tx_byte`, `write_tx_byte`, and `read_rx_byte` helpers keep each test case focused on intent rather than bit-level mechanics.

**Include edge cases.** The idle state test, reset test, and frame error test exercise conditions that are easy to miss in a design review but critical for correctness. Real hardware will encounter all of these.

**Keep timeouts finite.** The `wait_for` helper has a maximum cycle count. Without it, a bug could cause a test to spin forever. In CI, a hung test wastes build minutes and gives no useful feedback.

---

## Build and Test

Your final project structure should look like this:

```
uart-tutorial/
  skalp.toml
  src/
    counter.sk
    uart_tx.sk
    uart_rx.sk
    uart_types.sk
    fifo.sk
    async_fifo.sk
    uart_top.sk
    uart_cmd.sk
    alu.sk
  tests/
    counter_test.rs      <- Chapter 10 (standalone example)
    uart_test.rs         <- Chapter 10 (running project)
```

Run the entire test suite:

```bash
skalp test
```

Expected output:

```
   Compiling uart-tutorial v0.1.0
   Simulating Counter (3 tests)
   Simulating UartTop (12 tests)

running 15 tests
test test_counter_counts ... ok
test test_counter_overflow ... ok
test test_counter_disable ... ok
test test_uart_tx_basic ... ok
test test_uart_tx_consecutive_bytes ... ok
test test_uart_rx_basic ... ok
test test_uart_rx_all_zeros ... ok
test test_uart_rx_all_ones ... ok
test test_uart_rx_multiple_bytes ... ok
test test_uart_loopback ... ok
test test_uart_loopback_multiple ... ok
test test_uart_fifo_buffering ... ok
test test_uart_rx_fifo_overflow ... ok
test test_uart_idle_state ... ok
test test_uart_reset_clears_state ... ok
test test_uart_frame_error ... ok
test test_uart_with_vcd_dump ... ok

test result: ok. 17 passed; 0 failed; 0 ignored
```

Run a single test by name:

```bash
skalp test test_uart_loopback
```

Generate VCD waveforms for all tests:

```bash
skalp test --vcd
```

The VCD files are saved to `build/waveforms/`. Open them in GTKWave:

```bash
gtkwave build/waveforms/uart_debug.vcd
```

Since skalp tests are standard Rust tests under the hood, you can also run them with `cargo test` directly if you need Rust-level control over test execution:

```bash
cargo test
cargo test -- --nocapture   # show println! output
cargo test test_uart_rx     # run all tests matching "rx"
```

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Create testbench | `Testbench::new("Entity")` | `let mut tb = Testbench::new("UartTop");` |
| Set input port | `tb.set("port", value)` | `tb.set("rx", 1);` |
| Read signal value | `tb.get("port")` | `let v = tb.get("tx");` |
| Assert port value | `tb.expect("port", value)` | `tb.expect("count", 42);` |
| Advance one cycle | `tb.clock()` | `tb.clock();` |
| Advance N cycles | `tb.run(n)` | `tb.run(434);` |
| Assert then deassert reset | `tb.reset(n)` | `tb.reset(2);` |
| Save waveform | `tb.save_vcd("file.vcd")` | `tb.save_vcd("debug.vcd");` |
| Test attribute | `#[test]` | `#[test] fn test_foo() { ... }` |
| Import testbench | `use skalp_test::Testbench;` | Top of every test file |
| Run all tests | `skalp test` | From project root |
| Run with waveforms | `skalp test --vcd` | Saves to `build/waveforms/` |
| Run single test | `skalp test <name>` | `skalp test test_uart_loopback` |
| Run via cargo | `cargo test` | Standard Rust test runner |

---

## Tutorial Complete

Over ten chapters, you have built a complete UART peripheral from scratch in skalp:

**Chapter 1** introduced entities, ports, signals, and the `on(clk.rise)` / combinational split. You built a counter.

**Chapter 2** added state machines. You built the UART transmitter with baud rate timing and shift register serialization.

**Chapter 3** built the UART receiver with mid-bit sampling and edge detection.

**Chapter 4** introduced arrays and generics. You built a parameterized FIFO and added buffering to the UART.

**Chapter 5** covered const generics and parameterization. The UART became fully configurable -- baud rate, FIFO depth, data width.

**Chapter 6** introduced structs. Configuration and status became structured types instead of loose bundles of signals.

**Chapter 7** added enums and pattern matching. FSM states became typed, match expressions became exhaustive, and the command parser decoded incoming bytes into safe enum values.

**Chapter 8** tackled clock domain crossing. Clock lifetimes made the compiler enforce CDC safety, and a dual-clock async FIFO connected the system bus to the UART baud domain.

**Chapter 9** added safety annotations: TMR voting for radiation hardening, `#[trace]` for debug visibility, and `#[breakpoint]` for halt-on-condition debugging.

**Chapter 10** -- this chapter -- built a complete Rust test suite that exercises every feature of the design.

The final design is a parameterized, dual-clock UART peripheral with:

- Transmitter and receiver with configurable baud rate
- 16-deep TX and RX FIFOs with full/empty flags
- Enum-driven state machines with exhaustive transitions
- Struct-based configuration and status ports
- Clock domain crossing with compile-time safety
- TMR safety mechanisms and debug annotations
- Parity support and framing error detection
- A Rust test suite with 17 tests covering normal operation, edge cases, and error conditions

This is the kind of IP block that forms the backbone of real hardware projects -- an interface controller that sits between a processor bus and the physical world.

### Where to Go Next

The tutorial covered the skalp language and workflow. For deeper topics, see:

- **[skalp project page](/projects/skalp/)** -- Compiler architecture, intermediate representations, and the lowering pipeline from skalp source to synthesizable SystemVerilog.
- **[Null Convention Logic](/blog/null-convention-logic/)** -- How skalp supports asynchronous circuit design with NCL, removing the global clock entirely.
- **[Design Patterns in Real skalp Code](/blog/skalp-design-patterns/)** -- Production patterns for arbitration, bus bridges, register files, and multi-port memories.
- **[GitHub repository](https://github.com/girivs82/skalp)** -- Source code, more examples, and issue tracker.

You have built a complete, parameterized, dual-clock UART peripheral with safety mechanisms and a full test suite -- the kind of IP block that forms the backbone of real hardware projects. Everything you have learned here scales to larger designs.
