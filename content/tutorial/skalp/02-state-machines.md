---
title: "Chapter 2: State Machines — UART Transmitter"
date: 2025-07-15
summary: "Build a complete UART transmitter with FSM states, baud rate timing, and shift register serialization. Covers state encoding, counter-based timing, bit-level data shifting, and forward references for combinational signals."
tags: ["skalp", "tutorial", "hdl", "hardware", "uart"]
weight: 2
ShowToc: true
aliases: ["/tutorial/02-state-machines/"]
---

## What This Chapter Teaches

The counter from Chapter 1 does the same thing every cycle: increment. Real hardware needs to do different things at different times — wait for a start command, send a start bit, shift out eight data bits, send a stop bit, then go back to waiting. This is a **finite state machine**, and FSMs are the backbone of digital control logic.

This chapter teaches you how to build FSMs in skalp using patterns you already know from Chapter 1. There is no special FSM keyword or construct. A state machine is just:

- A `signal` that holds the current state (an integer)
- An `on(clk.rise)` block with `if-else` chains that check the state and decide what to do
- Counters for timing (how long to stay in each state)
- Combinational signals for derived values like "baud tick"

By the end of this chapter you will understand:

- How to encode FSM states as integer constants in a `signal`
- How to structure state transitions with nested `if-else` inside `on(clk.rise)`
- How to build a baud rate counter that generates periodic tick signals
- How to use a shift register to serialize parallel data into a serial bitstream
- How combinational forward references let you define a `baud_tick` signal after using it
- How to initialize signals to known values on reset

You will build two things: a standalone traffic light controller (simple, three states) and the UART transmitter that becomes a permanent part of the running project.

---

## Standalone Example: Traffic Light Controller

Before tackling the UART, let us build a minimal FSM to see the pattern clearly. A traffic light cycles through three states — Red, Yellow, Green — each held for a fixed number of clock cycles.

Create `src/traffic_light.sk`:

```
// Traffic light controller with three states and configurable timing.
//
// Assumes a 50 MHz clock. Each state holds for a fixed duration:
//   Red:    3 seconds  (150,000,000 cycles)
//   Yellow: 0.5 seconds (25,000,000 cycles)
//   Green:  2 seconds  (100,000,000 cycles)
//
// For simulation, use much shorter values.

entity TrafficLight {
    in clk: clock,
    in rst: reset,
    out red: bit[1],
    out yellow: bit[1],
    out green: bit[1]
}

impl TrafficLight {
    // State encoding — just integer constants.
    // 0 = Red, 1 = Yellow, 2 = Green.
    signal state: nat[2]

    // Timer counts down in the current state.
    // 28 bits holds up to 268 million — enough for 5 seconds at 50 MHz.
    signal timer: nat[28]

    // Duration constants. In a real design these would be generic
    // parameters (Chapter 5). For now, hardcode them.
    // Using short values here so simulation completes quickly.
    //
    // Production values:
    //   RED_DURATION    = 150_000_000
    //   YELLOW_DURATION =  25_000_000
    //   GREEN_DURATION  = 100_000_000

    on(clk.rise) {
        if rst {
            state = 0    // start in Red
            timer = 1000 // reset timer to Red duration
        } else {
            if timer > 0 {
                // Stay in current state, count down.
                timer = timer - 1
            } else {
                // Timer expired — transition to next state.
                if state == 0 {
                    // Red -> Green
                    state = 2
                    timer = 800
                } else if state == 2 {
                    // Green -> Yellow
                    state = 1
                    timer = 200
                } else {
                    // Yellow -> Red
                    state = 0
                    timer = 1000
                }
            }
        }
    }

    // Combinational outputs — active based on current state.
    red    = (state == 0)
    yellow = (state == 1)
    green  = (state == 2)
}
```

### The FSM Pattern

This is the entire pattern for FSMs in skalp:

1. **Declare state**: `signal state: nat[2]` — wide enough to hold all state values.
2. **Declare timer**: `signal timer: nat[28]` — wide enough for the longest duration.
3. **Reset block**: Set initial state and timer on reset.
4. **Timer logic**: If the timer is not zero, decrement it. The FSM stays in its current state.
5. **Transition logic**: When the timer hits zero, check the current state and move to the next one, loading the new timer value.
6. **Combinational outputs**: Drive outputs based on the current state, outside the `on` block.

There is no `case` or `match` here — just `if-else`. For three states, `if-else` is clear enough. When you have more states, Chapter 7 introduces `match` expressions with exhaustiveness checking. For now, `if-else` works and is easy to read.

> **Coming from SystemVerilog?**
>
> This is structurally identical to a SystemVerilog FSM with `always_ff` and a `case` statement. The differences are:
>
> - No `enum` declaration needed for states (though skalp has enums — Chapter 7). Integer constants work fine for simple FSMs.
> - No `default` branch that silently swallows unhandled states. If you add a fourth state and forget to handle it, the `else` branch catches it — but that is a deliberate choice, not an accident.
> - The combinational outputs (`red = (state == 0)`) look like `assign` statements but need no keyword. They are outside the `on` block, so they are combinational by definition.
> - No `wire` vs `reg` confusion. `state` and `timer` are registers because they are assigned inside `on(clk.rise)`. `red`, `yellow`, `green` are combinational because they are assigned outside it. The compiler knows this from context.

---

## Running Project: UART Transmitter

Now for the real thing. The UART transmitter serializes an 8-bit byte into a 10-bit frame: one start bit (low), eight data bits (LSB first), and one stop bit (high). The line idles high when no transmission is active.

At 115200 baud with a 50 MHz clock, each bit lasts 434 clock cycles (50,000,000 / 115,200 = 434.03, truncated to 434). A baud counter counts these 434 cycles, and a "baud tick" signal pulses once per bit period to advance the FSM.

### The Full UART TX

Create `src/uart_tx.sk`:

```skalp
// UART Transmitter — 8N1 (8 data bits, no parity, 1 stop bit)
//
// Protocol:
//   IDLE:  tx line held high
//   START: tx driven low for one bit period (434 cycles)
//   DATA:  8 data bits transmitted LSB first, one bit period each
//   STOP:  tx driven high for one bit period
//
// Interface:
//   tx_data  — the byte to transmit (active when tx_start pulses)
//   tx_start — pulse high for one cycle to begin transmission
//   tx       — the serial output line
//   tx_busy  — high while a transmission is in progress
//
// Timing:
//   50 MHz clock / 115200 baud = 434 cycles per bit
//   Total frame: 434 * 10 = 4340 cycles (~86.8 us)

entity UartTx {
    in clk: clock       // 50MHz system clock
    in rst: reset

    // Data interface
    in tx_start: bit    // Pulse to start transmission
    in tx_data: nat[8]  // 8-bit data to transmit
    out tx_busy: bit    // High when transmitting

    // UART interface
    out tx: bit         // Serial output line
}

impl UartTx {
    // Baud rate timing: 50MHz / 115200 = 434 clocks per bit
    signal baud_counter: nat[9] = 0
    signal bit_counter: nat[4] = 0

    // State encoding
    signal state: nat[2] = 0
    // 0 = IDLE
    // 1 = START_BIT
    // 2 = DATA_BITS
    // 3 = STOP_BIT

    // Shift register for data
    signal shift_reg: nat[8] = 0

    // TX output defaults to idle (high)
    signal tx_out: bit = 1
    tx = tx_out

    // Busy when not in IDLE state
    tx_busy = (state != 0)

    on(clk.rise) {
        if (rst) {
            state = 0
            baud_counter = 0
            bit_counter = 0
            shift_reg = 0
            tx_out = 1
        } else {
            // Default: decrement baud counter if active.
            // This runs every cycle. State transitions below
            // override baud_counter when they reload it.
            if (baud_counter > 0) {
                baud_counter = baud_counter - 1
            }

            // State machine
            if (state == 0) {
                // IDLE state
                tx_out = 1
                if (tx_start) {
                    // Start transmission
                    state = 1
                    shift_reg = tx_data
                    baud_counter = 434
                    bit_counter = 0
                }
            } else if (state == 1) {
                // START_BIT state
                tx_out = 0
                if (baud_counter == 0) {
                    state = 2
                    baud_counter = 434
                }
            } else if (state == 2) {
                // DATA_BITS state
                // Output LSB of shift register
                tx_out = shift_reg % 2

                if (baud_counter == 0) {
                    // Shift to next bit
                    shift_reg = shift_reg / 2
                    bit_counter = bit_counter + 1

                    if (bit_counter == 7) {
                        // All 8 bits sent
                        state = 3
                    }
                    baud_counter = 434
                }
            } else {
                // STOP_BIT state
                tx_out = 1
                if (baud_counter == 0) {
                    state = 0
                }
            }
        }
    }
}
```

### How the UART TX Works

**Default-decrement pattern.** The `baud_counter` uses a common RTL pattern: a default operation runs every cycle, and specific states override it when needed. The line `if (baud_counter > 0) { baud_counter = baud_counter - 1 }` at the top of the else block runs every cycle, automatically counting down. State transitions reload the counter to 434 when they need a new bit period. This pattern eliminates the need for explicit `else { baud_counter = baud_counter - 1 }` in every state — the default handles it. In skalp, when multiple assignments to the same signal exist in the same `on` block, the *last assignment wins*. The state machine assignments execute after the default, so they override when needed.

**State transitions.** The FSM has four states:

| State | Value | TX line | Duration | Next state |
|-------|-------|---------|----------|------------|
| IDLE  | 0 | High (1) | Until `tx_start` | START |
| START | 1 | Low (0)  | 434 cycles | DATA |
| DATA  | 2 | `shift_reg % 2` | 434 cycles per bit, 8 bits | STOP |
| STOP  | 3 | High (1) | 434 cycles | IDLE |

**Data serialization.** When `tx_start` pulses, the byte is latched into `shift_reg`. During the DATA state, `tx_out` is set to `shift_reg % 2` — this extracts the least significant bit. When the baud counter reaches zero, `shift_reg = shift_reg / 2` shifts the data right by one position, exposing the next bit. After 8 bit periods (bit_counter 0 through 7), all bits have been sent.

**Bit ordering.** UART sends LSB first. Dividing by 2 and taking modulo 2 naturally produces this ordering: bit 0 is sent first, bit 7 is sent last. The expressions `shift_reg % 2` and `shift_reg / 2` are equivalent to bit-select and right-shift — the compiler generates the same hardware.

**Registered output.** The `tx` line is driven from a register (`tx_out`) rather than a combinational expression. Each state explicitly sets `tx_out` to the correct value: 1 in IDLE and STOP, 0 in START, and the current LSB in DATA. The combinational assignment `tx = tx_out` wires this register to the output port. Using a registered output avoids glitches on the serial line — the value changes cleanly on the clock edge.

### Signals at a Glance

| Signal | Type | Kind | Purpose |
|--------|------|------|---------|
| `state` | `nat[2]` | Register | FSM state (0-3) |
| `baud_counter` | `nat[9]` | Register | Counts 434 cycles per bit period |
| `bit_counter` | `nat[4]` | Register | Tracks which data bit (0-7) is being sent |
| `shift_reg` | `nat[8]` | Register | Holds the byte, divides by 2 to shift right |
| `tx_out` | `bit` | Register | Drives the serial output, set per-state |
| `tx` | `bit` | Combinational output | Serial line (wired from `tx_out`) |
| `tx_busy` | `bit` | Combinational output | High during transmission |

> **Coming from SystemVerilog?**
>
> The UART TX in skalp and SystemVerilog are structurally the same — `always_ff` becomes `on(clk.rise)`, `case` becomes `if-else`, `assign` becomes bare assignment. Three things are worth noting:
>
> 1. **Default assignment pattern.** The `if (baud_counter > 0) { baud_counter = baud_counter - 1 }` at the top of the else block is equivalent to placing a default assignment at the top of an `always_ff` block in SystemVerilog. Later assignments in the same block override it. This is a standard RTL pattern for "decrement unless told otherwise."
>
> 2. **No wire/reg declaration.** In SystemVerilog you would write `reg [1:0] state` and `wire tx_busy` and get a tool error if you mixed them up. In skalp, `signal state: nat[2]` becomes a register because it is assigned inside `on(clk.rise)`. `tx_busy` becomes combinational because it is assigned outside. The compiler infers the distinction from usage.
>
> 3. **Arithmetic for bit manipulation.** The expressions `shift_reg % 2` and `shift_reg / 2` extract the LSB and shift right, respectively. The compiler maps these to the same hardware as `shift_reg[0]` and `shift_reg >> 1`. You can use either style — arithmetic or bitwise — depending on what reads more naturally for your use case.

---

## Build and Test

### Building the UART TX

Update `skalp.toml` to set the new top entity:

```toml
[package]
name = "uart-tutorial"
version = "0.1.0"

[build]
top = "UartTx"
```

Build:

```bash
skalp build
```

Expected output:

```
   Compiling uart-tutorial v0.1.0
   Analyzing UartTx
       Built UartTx -> build/uart_tx.sv
```

### Inspecting a Byte Transmission

To capture waveforms of a byte transmission, add `tb.export_waveform("build/uart_tx.skw.gz").unwrap();` at the end of a test. Open the `.skw.gz` file in the skalp VS Code extension. Using 0x55 (01010101 in binary) as a test pattern (useful because the bits alternate), you should see:

1. **Reset phase** (cycles 0-10): `state` = 0, `tx` = 1 (idle high)
2. **Start bit** (cycles ~11-444): `tx` drops to 0 for 434 cycles
3. **Data bits** (cycles ~445-3916): `tx` alternates 1-0-1-0-1-0-1-0 (0x55 LSB first), each bit held for 434 cycles
4. **Stop bit** (cycles ~3917-4350): `tx` goes to 1 for 434 cycles
5. **tx_busy** returns to 0 when the FSM enters IDLE

The total frame takes 4340 cycles (10 bit periods at 434 cycles each).

### Checking the Generated SystemVerilog

Inspect `build/uart_tx.sv`. You will see a standard SystemVerilog module with `always_ff @(posedge clk)` for the sequential logic and `assign` statements for the combinational outputs. The compiler flattens the if-else chain into a priority-encoded structure. The generated code is synthesizable with any standard tool (Vivado, Quartus, Yosys).

---

## Testing Your Design

State machines are notoriously error-prone — off-by-one timer values, missing transitions, wrong initial states. A testbench catches these issues before they reach hardware. The basic testing pattern is `tb.set()` inputs, `tb.clock(n).await` forward, then `tb.expect().await` outputs.

Here are tests for the two entities in this chapter (from `tests/ch02_test.rs`):

### TrafficLight

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_traffic_light_initial_state() {
    let mut tb = Testbench::with_top_module("src/traffic_light.sk", "TrafficLight")
        .await.unwrap();
    tb.reset(2).await;

    // After reset, should start in Red state
    tb.expect("red", 1u32).await;
    tb.expect("yellow", 0u32).await;
    tb.expect("green", 0u32).await;
}

#[tokio::test]
async fn test_traffic_light_full_cycle() {
    let mut tb = Testbench::with_top_module("src/traffic_light.sk", "TrafficLight")
        .await.unwrap();
    tb.reset(2).await;

    // Should be in Red initially
    tb.expect("red", 1u32).await;

    // Run through Red duration (1001 cycles: 1000 countdown + 1 transition)
    tb.clock(1001).await;

    // Should now be in Green (Red -> Green transition)
    tb.expect("red", 0u32).await;
    tb.expect("green", 1u32).await;
    tb.expect("yellow", 0u32).await;

    // Run through Green duration
    tb.clock(801).await;

    // Should now be in Yellow
    tb.expect("yellow", 1u32).await;
}
```

### UartTx

```rust
#[tokio::test]
async fn test_uart_tx_single_byte() {
    let mut tb = Testbench::with_top_module("src/uart_tx.sk", "UartTx")
        .await.unwrap();
    tb.reset(2).await;

    // After reset, TX line should be idle high
    assert_eq!(tb.get_u64("tx").await, 1);
    tb.expect("tx_busy", 0u32).await;

    // Send byte 0x55 (alternating bits)
    tb.set("tx_data", 0x55u32);
    tb.set("tx_start", 1u8);
    tb.clock(1).await;
    tb.set("tx_start", 0u8);

    // Should be busy now
    tb.expect("tx_busy", 1u32).await;

    let captured = capture_tx_bits(&mut tb).await;
    assert_eq!(captured, 0x55);
}

#[tokio::test]
async fn test_uart_tx_busy_flag() {
    let mut tb = Testbench::with_top_module("src/uart_tx.sk", "UartTx")
        .await.unwrap();
    tb.reset(2).await;

    tb.expect("tx_busy", 0u32).await;  // idle

    tb.set("tx_data", 0xAAu32);
    tb.set("tx_start", 1u8);
    tb.clock(1).await;
    tb.set("tx_start", 0u8);

    tb.expect("tx_busy", 1u32).await;  // transmitting

    tb.clock(434 * 10 + 100).await;    // wait for full frame
    tb.expect("tx_busy", 0u32).await;  // done
}
```

Run the tests with:

```bash
cargo test
```

**Exercise:** Write a `capture_tx_bits` async helper function that waits for the start bit (TX goes low), then samples 8 data bits at mid-bit points (every 434 cycles from the middle of the start bit), and returns the captured byte. Use it to verify that `0xAA` transmits correctly.

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| State signal | `signal name: nat[N]` | `signal state: nat[2]` (4 states: 0-3) |
| State transition | `if state == N { state = M }` | Inside `on(clk.rise)` |
| Default decrement | `if (counter > 0) { counter = counter - 1 }` | At top of else block, overridden by states |
| Counter reload | `counter = VALUE` | `baud_counter = 434` |
| LSB extraction | `reg % 2` | `shift_reg % 2` (equivalent to `shift_reg[0]`) |
| Shift right | `reg = reg / 2` | `shift_reg = shift_reg / 2` (equivalent to `>> 1`) |
| Not equal | `!=` | `tx_busy = (state != 0)` |
| Signal initialization | `signal name: type = value` | `signal tx_out: bit = 1` |
| Reset assignment | `signal = value` inside `if rst` | `state = 0` on reset |

---

## Next: UART Receiver

The transmitter sends bytes out. In Chapter 3, you will build the receiver that reads them back in. The UART RX is more challenging because it must:

- Detect the falling edge of the start bit on an asynchronous input
- Sample each data bit at the *middle* of the bit period (not the edge) for maximum noise margin
- Reconstruct the 8-bit byte from the serial stream
- Validate the stop bit and signal framing errors

You will learn edge detection patterns, mid-bit sampling with half-period counters, and how to assemble serial bits back into a parallel byte.

The TX and RX together form a complete UART data path. Later chapters will add FIFOs for buffering (Chapter 4), parameterization for configurable baud rates (Chapter 5), and structured ports to bundle the interface cleanly (Chapter 6).

Continue to [Chapter 3: UART Receiver](../03-uart-receiver/).
