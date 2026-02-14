---
title: "Chapter 2: State Machines — UART Transmitter"
date: 2025-07-15
summary: "Build a complete UART transmitter with FSM states, baud rate timing, and shift register serialization. Covers state encoding, counter-based timing, bit-level data shifting, and forward references for combinational signals."
tags: ["skalp", "tutorial", "hdl", "hardware", "uart"]
weight: 2
ShowToc: true
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

```
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
//   tx_done  — pulses high for one cycle when transmission completes
//
// Timing:
//   50 MHz clock / 115200 baud = 434 cycles per bit
//   Total frame: 434 * 10 = 4340 cycles (~86.8 us)

entity UartTx {
    in clk: clock,
    in rst: reset,
    in tx_data: bit[8],
    in tx_start: bit[1],
    out tx: bit[1],
    out tx_busy: bit[1],
    out tx_done: bit[1]
}

impl UartTx {
    // ---------------------------------------------------------------
    // Constants
    // ---------------------------------------------------------------
    // Cycles per bit at 50 MHz / 115200 baud.
    // In Chapter 5 this becomes a generic parameter.
    // CYCLES_PER_BIT = 434

    // ---------------------------------------------------------------
    // State encoding
    // ---------------------------------------------------------------
    // 0 = IDLE   — line high, waiting for tx_start
    // 1 = START  — driving start bit (low)
    // 2 = DATA   — shifting out 8 data bits
    // 3 = STOP   — driving stop bit (high)
    signal state: nat[2]

    // ---------------------------------------------------------------
    // Baud rate generator
    // ---------------------------------------------------------------
    // Counts from 433 down to 0, then reloads. Generates a single-
    // cycle baud_tick pulse when it hits 0.
    signal baud_counter: nat[9]

    // ---------------------------------------------------------------
    // Data tracking
    // ---------------------------------------------------------------
    // Which bit (0-7) we are currently transmitting.
    signal bit_index: nat[3]

    // Shift register — holds the byte being transmitted. We shift
    // right and send the LSB on each baud tick.
    signal shift_reg: bit[8]

    // ---------------------------------------------------------------
    // Baud tick — combinational, defined here but used below.
    // Forward reference: skalp allows this because combinational
    // signals have no temporal ordering.
    // ---------------------------------------------------------------
    baud_tick = (baud_counter == 0)

    // ---------------------------------------------------------------
    // Sequential logic — the main FSM
    // ---------------------------------------------------------------
    on(clk.rise) {
        if rst {
            state = 0          // IDLE
            baud_counter = 0
            bit_index = 0
            shift_reg = 0
        } else {
            // ------ IDLE (state == 0) ------
            if state == 0 {
                // Line idles high. Wait for tx_start.
                if tx_start {
                    // Latch the data byte and begin transmission.
                    shift_reg = tx_data
                    state = 1          // -> START
                    baud_counter = 433 // load full bit period
                    bit_index = 0
                }

            // ------ START (state == 1) ------
            } else if state == 1 {
                // Driving the start bit (low). Wait for one full
                // bit period, then move to DATA.
                if baud_tick {
                    state = 2          // -> DATA
                    baud_counter = 433
                } else {
                    baud_counter = baud_counter - 1
                }

            // ------ DATA (state == 2) ------
            } else if state == 2 {
                // Sending data bits LSB first. On each baud tick,
                // shift right and advance the bit index.
                if baud_tick {
                    if bit_index == 7 {
                        // Last data bit sent. Move to STOP.
                        state = 3          // -> STOP
                        baud_counter = 433
                    } else {
                        // More bits to send. Shift and continue.
                        shift_reg = shift_reg >> 1
                        bit_index = bit_index + 1
                        baud_counter = 433
                    }
                } else {
                    baud_counter = baud_counter - 1
                }

            // ------ STOP (state == 3) ------
            } else {
                // Driving the stop bit (high). Wait one full bit
                // period, then return to IDLE.
                if baud_tick {
                    state = 0  // -> IDLE
                } else {
                    baud_counter = baud_counter - 1
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // Combinational outputs
    // ---------------------------------------------------------------

    // tx line value depends on the current state.
    // IDLE and STOP: line high (1)
    // START: line low (0)
    // DATA: current LSB of shift register
    tx = if state == 0 {
        1
    } else if state == 1 {
        0
    } else if state == 2 {
        shift_reg[0]
    } else {
        1
    }

    // Busy whenever not idle.
    tx_busy = (state != 0)

    // Done pulses for one cycle at the end of the stop bit.
    tx_done = (state == 3) & baud_tick
}
```

### How the UART TX Works

**Baud rate timing.** The `baud_counter` counts from 433 down to 0. When it hits 0, `baud_tick` goes high for exactly one cycle. This tick drives all state transitions — no state change happens mid-bit. The counter reloads to 433 at every transition. The value 433 (not 434) is correct because counting from 433 to 0 inclusive is 434 cycles.

**State transitions.** The FSM has four states:

| State | Value | TX line | Duration | Next state |
|-------|-------|---------|----------|------------|
| IDLE  | 0 | High (1) | Until `tx_start` | START |
| START | 1 | Low (0)  | 434 cycles | DATA |
| DATA  | 2 | `shift_reg[0]` | 434 cycles per bit, 8 bits | STOP |
| STOP  | 3 | High (1) | 434 cycles | IDLE |

**Data serialization.** When `tx_start` pulses, the byte is latched into `shift_reg`. During the DATA state, each baud tick shifts the register right by one, exposing the next bit at position 0. The `tx` output reads `shift_reg[0]` — the current LSB. After 8 ticks (bit_index 0 through 7), all bits have been sent.

**Bit ordering.** UART sends LSB first. Shifting right naturally produces this ordering: bit 0 is sent first, bit 7 is sent last.

**Output encoding.** The `tx` output is a combinational if-else expression. In the IDLE and STOP states, it is 1 (high — the idle level). In the START state, it is 0 (the start bit). In the DATA state, it is the current LSB of the shift register. This is a multiplexer, selected by the state.

**Forward reference.** Notice that `baud_tick` is defined as a combinational signal near the top of the impl, then used inside the `on(clk.rise)` block. This works because combinational signals have no temporal ordering — they are continuous functions of their inputs, evaluated every cycle. You could move the `baud_tick = ...` line to the very bottom of the impl and it would behave identically. The compiler resolves the dependency graph regardless of source order.

### Signals at a Glance

| Signal | Type | Kind | Purpose |
|--------|------|------|---------|
| `state` | `nat[2]` | Register | FSM state (0-3) |
| `baud_counter` | `nat[9]` | Register | Counts 434 cycles per bit (0-433) |
| `bit_index` | `nat[3]` | Register | Tracks which data bit (0-7) is being sent |
| `shift_reg` | `bit[8]` | Register | Holds the byte, shifts right each baud tick |
| `baud_tick` | implicit `bit[1]` | Combinational | Pulses when baud_counter reaches 0 |
| `tx` | `bit[1]` | Combinational output | Serial line |
| `tx_busy` | `bit[1]` | Combinational output | High during transmission |
| `tx_done` | `bit[1]` | Combinational output | One-cycle pulse at end of stop bit |

> **Coming from SystemVerilog?**
>
> The UART TX in skalp and SystemVerilog are structurally the same — `always_ff` becomes `on(clk.rise)`, `case` becomes `if-else`, `assign` becomes bare assignment. Three things are worth noting:
>
> 1. **Forward references.** In SystemVerilog, `baud_tick` would need to be declared as a `wire` and assigned with `assign` *before* the `always_ff` block that uses it — or at least before it in the file. In skalp, the combinational assignment can appear anywhere in the impl. This lets you put `baud_tick` near the baud counter logic where it conceptually belongs, rather than forcing a declaration-order constraint.
>
> 2. **No wire/reg declaration.** In SystemVerilog you would write `reg [1:0] state` and `wire baud_tick` and get a tool error if you mixed them up. In skalp, `signal state: nat[2]` becomes a register because it is assigned inside `on(clk.rise)`. `baud_tick` becomes combinational because it is assigned outside. The compiler infers the distinction from usage.
>
> 3. **Expression-based tx output.** The `tx = if state == 0 { 1 } else if ...` construct is an expression that returns a value, not a statement. In SystemVerilog you would use nested ternaries (`assign tx = (state == 0) ? 1 : (state == 1) ? 0 : ...`) or a combinational `always_comb` with a `case`. The skalp version reads more like structured code.

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

### Simulating a Byte Transmission

Run a simulation that sends one byte (0x55 = 01010101 in binary, a useful test pattern because the bits alternate):

```bash
skalp sim --entity UartTx --cycles 5000 --vcd build/uart_tx.vcd
```

In the waveform viewer, you should see:

1. **Reset phase** (cycles 0-10): `state` = 0, `tx` = 1 (idle high)
2. **Start bit** (cycles ~11-444): `tx` drops to 0 for 434 cycles
3. **Data bits** (cycles ~445-3916): `tx` alternates 1-0-1-0-1-0-1-0 (0x55 LSB first), each bit held for 434 cycles
4. **Stop bit** (cycles ~3917-4350): `tx` goes to 1 for 434 cycles
5. **tx_done** pulses high for one cycle at the end of the stop bit
6. **tx_busy** returns to 0 when the FSM enters IDLE

The total frame takes 4340 cycles (10 bit periods at 434 cycles each).

### Checking the Generated SystemVerilog

Inspect `build/uart_tx.sv`. You will see a standard SystemVerilog module with `always_ff @(posedge clk)` for the sequential logic and `assign` statements for the combinational outputs. The compiler flattens the if-else chain into a priority-encoded structure. The generated code is synthesizable with any standard tool (Vivado, Quartus, Yosys).

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| State signal | `signal name: nat[N]` | `signal state: nat[2]` (4 states: 0-3) |
| State transition | `if state == N { state = M }` | Inside `on(clk.rise)` |
| Counter (count down) | `counter = counter - 1` | `baud_counter = baud_counter - 1` |
| Counter reload | `counter = VALUE` | `baud_counter = 433` |
| Tick signal | `tick = (counter == 0)` | `baud_tick = (baud_counter == 0)` |
| Shift right | `reg = reg >> 1` | `shift_reg = shift_reg >> 1` |
| Bit index access | `signal[N]` | `shift_reg[0]` (LSB) |
| If-else expression | `if cond { val } else { val }` | `tx = if state == 0 { 1 } else { 0 }` |
| Not equal | `!=` | `tx_busy = (state != 0)` |
| Bitwise AND | `&` | `tx_done = (state == 3) & baud_tick` |
| Forward reference | Use before define | `baud_tick` used in `on`, defined after |
| Signal initialization | `signal = value` inside `if rst` | `state = 0` on reset |

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
