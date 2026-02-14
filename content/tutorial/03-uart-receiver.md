---
title: "Chapter 3: UART Receiver"
date: 2025-07-15
summary: "Mid-bit sampling, edge detection, and bit reconstruction — build the RX side of the UART."
tags: ["skalp", "tutorial", "uart", "fsm", "edge-detection"]
weight: 3
ShowToc: true
---

## What You'll Learn

In Chapter 2, you built a UART transmitter that serialises bytes onto a wire. Now you need the other half: a **receiver** that watches an incoming serial line, detects when a byte is arriving, samples each bit at the right moment, and reassembles the original byte.

This chapter introduces three essential patterns for digital design in skalp:

- **Edge detection** — comparing the current value of a signal to its value one clock cycle ago to find transitions.
- **Mid-bit sampling** — starting a counter on the detected edge, then reading the line at the halfway point of each bit period where the signal is most stable.
- **Bit reconstruction** — shifting sampled bits into a register to rebuild the original byte.

These three patterns appear everywhere in digital design, not just in UARTs. Any time you need to respond to an asynchronous input — a button press, an SPI clock, a handshake signal from another clock domain — you will use some variation of edge detection. Any time you need to recover data from a noisy or timing-uncertain signal, mid-bit sampling is the standard approach.

We will start with a standalone **button debouncer** that demonstrates counter-based stability detection, then apply the same principles to build a full UART receiver and wire it up alongside the transmitter from Chapter 2.

By the end of this chapter, your running project will include:

- `src/debouncer.sk` — standalone example of edge detection and stability counting
- `src/uart_rx.sk` — the UART receiver module
- `src/uart_loopback.sk` — a loopback test harness that connects TX to RX

---

## Standalone Example: Button Debouncer

Before jumping into UART RX, let us look at a smaller problem that shares the core idea: you have an input signal that can be noisy, and you need to decide when it has genuinely changed.

A mechanical push-button bounces when pressed or released, producing rapid high-low transitions for a few milliseconds. If you use the raw signal directly, a single button press might register as dozens of presses. A debouncer solves this by counting how many consecutive clock cycles the button has been in the same state, and only updating its output after the count reaches a threshold.

The debouncer demonstrates the same fundamental technique the UART receiver uses: store the previous value of an input, compare it to the current value, and use a counter to decide when to act.

Create `src/debouncer.sk`:

```skalp
// debouncer.sk — output changes only after N consecutive stable readings

entity Debouncer {
    in  clk:       clock
    in  rst:       reset
    in  btn_raw:   bit
    out btn_clean: bit
}

impl Debouncer {
    // Threshold: 1 million cycles at 50 MHz is 20 ms — more than enough
    // to ride out mechanical bounce. Typical bounce duration is 1-5 ms,
    // so 20 ms gives us a generous safety margin.
    signal STABLE_THRESHOLD: nat[20] = 1_000_000

    // Counter tracks how long btn_raw has held its current value.
    // 20 bits can count up to 1,048,575 — just enough for our threshold.
    signal counter: nat[20]

    // The last raw value we saw — used to detect changes.
    signal btn_prev: bit

    // The debounced output. We declare it here and assign inside the
    // on block. It becomes a register because it is assigned
    // sequentially.
    signal debounced: bit

    on(clk.rise) {
        // Capture the current raw value for comparison next cycle.
        // After this assignment, btn_prev holds the "previous" value
        // and btn_raw holds the "current" value.
        btn_prev = btn_raw

        if rst {
            counter   = 0
            debounced = 0
            btn_prev  = 0
        } else if btn_raw != btn_prev {
            // Input changed — restart the counter. This is the edge
            // detection: any time the raw input differs from its
            // one-cycle-delayed copy, we know a transition occurred.
            counter = 0
        } else if counter < STABLE_THRESHOLD {
            // Input is stable but we haven't reached the threshold yet.
            // Keep counting.
            counter = counter + 1
        } else {
            // Stable for long enough — update the output. This only
            // happens once: after the counter reaches the threshold,
            // the else-if chain falls through and the counter stops
            // incrementing (it stays at STABLE_THRESHOLD).
            debounced = btn_raw
        }
    }

    // Drive the output port from the registered debounced value.
    // This is a combinational assignment — it's outside the on block,
    // so it acts like a continuous wire from the register to the port.
    btn_clean = debounced
}
```

### Why This Works

The pattern here is straightforward but worth understanding in detail, because the UART receiver uses the exact same structure.

**Previous-value register.** `btn_prev` is updated every clock cycle with the current value of `btn_raw`. This means `btn_prev` always holds what `btn_raw` was *last cycle*. When `btn_raw != btn_prev`, the input has just changed — this is edge detection.

**Stability counter.** Once we see a change, we reset the counter to zero and start counting. If the input changes again (another bounce), the counter resets again. Only when the input has been rock-steady for `STABLE_THRESHOLD` consecutive cycles do we accept the new value.

**Output gating.** The debounced output only updates when the counter reaches the threshold. Between bounces, it holds its previous value. This is exactly the behaviour we want: the output changes once per physical press, not once per electrical bounce.

**Counter saturation.** Notice that once `counter` reaches `STABLE_THRESHOLD`, neither the increment branch nor the reset branch fires (assuming the input stays stable). The counter just stays at `STABLE_THRESHOLD`. This is implicit saturation — no overflow, no wasted logic toggling a counter that has already done its job.

---

> **Coming from SystemVerilog?**
>
> The debouncer above is structurally identical to what you would write in SV — an `always_ff` block with a counter and a previous-value register. Three things to note:
>
> - In skalp, `btn_clean = debounced` outside the `on` block is a **combinational** assignment (like `assign`). Inside the `on` block, assignments are **sequential** (like `always_ff`). You never write `assign` or `always_comb` explicitly; the position of the assignment determines its semantics. This removes a common class of SV bugs where a signal is accidentally assigned in the wrong process type.
> - You can **forward-reference** combinational signals. If `debounced` were used in a combinational expression before being assigned, skalp resolves it. No need to pre-declare wires or worry about ordering. The compiler builds a dependency graph and reports true combinational loops as errors.
> - `nat[20]` gives you a 20-bit unsigned value. No `[19:0]` — just the bit-width directly. This matches how you think about the signal ("I need 20 bits") rather than how it is indexed ("bits 19 down to 0").

---

## Running Project: UART Receiver

Now we build the real thing. The UART RX module watches the `rx` serial line, detects the start bit, samples each data bit at mid-period, and outputs the reconstructed byte with a one-cycle valid pulse.

### The Reception Problem

Receiving serial data is fundamentally harder than transmitting it. The transmitter controls the timing — it decides exactly when each bit begins and ends. The receiver has no such luxury. It must:

1. Detect the *start* of a frame from an asynchronous signal that could transition at any time.
2. Figure out where the *middle* of each bit period is, so it samples where the signal is most stable.
3. Maintain synchronisation across all 10 bit periods (start + 8 data + stop) without drifting.

The standard solution is **mid-bit sampling**: detect the falling edge of the start bit, wait half a bit period to reach the centre of the start bit, then wait full bit periods to reach the centre of each subsequent bit.

### UART Frame Recap

Recall the UART frame from Chapter 2:

```
    IDLE  START  D0  D1  D2  D3  D4  D5  D6  D7  STOP  IDLE
rx: ‾‾‾‾‾‾╲____╱‾‾‾╱‾‾‾╱‾‾‾╱‾‾‾╱‾‾‾╱‾‾‾╱‾‾‾╱‾‾‾╱‾‾‾‾‾‾‾‾‾‾
              ^   ^   ^   ^   ^   ^   ^   ^   ^   ^
              |   sample points at mid-bit
              |
              falling edge triggers reception
```

The receiver's job, step by step:

1. Wait in IDLE, watching for a **falling edge** on `rx` (the start bit pulls the line low).
2. Enter the START state and count to the **mid-point** of the start bit (half a bit period = 217 cycles at 50 MHz / 115200 baud).
3. Verify `rx` is still low at mid-start-bit. If it is high, the falling edge was noise — return to IDLE.
4. Enter the DATA state. For each of the 8 data bits, count a **full bit period** (434 cycles), then sample `rx` at mid-bit and shift it into the receive shift register.
5. After all 8 bits, enter the STOP state, wait one bit period, and verify `rx` is high (the stop bit). Pulse `rx_valid` to signal that a complete byte is available.

### The Implementation

Create `src/uart_rx.sk`:

```skalp
// uart_rx.sk — UART receiver with mid-bit sampling

entity UartRx {
    in  clk:      clock
    in  rst:      reset
    in  rx:       bit          // serial input line
    out rx_data:  bit[8]       // received byte
    out rx_valid: bit          // pulses high for one cycle on complete byte
}

impl UartRx {
    // Baud rate parameters — same as the transmitter.
    // 50 MHz / 115200 = 434 cycles per bit.
    signal CYCLES_PER_BIT: nat[9] = 434
    signal HALF_BIT:       nat[9] = 217

    // FSM states — same encoding scheme as the transmitter.
    // Two bits give us four states, which is exactly what we need.
    signal IDLE:  nat[2] = 0
    signal START: nat[2] = 1
    signal DATA:  nat[2] = 2
    signal STOP:  nat[2] = 3

    // State register
    signal state: nat[2]

    // Baud counter — counts clock cycles within the current bit period.
    // 9 bits can hold values 0..511, comfortably covering our maximum
    // count of 434.
    signal baud_counter: nat[9]

    // Bit index — which data bit we're receiving (0 through 7).
    // 3 bits is exactly right for indexing 8 positions.
    signal bit_index: nat[3]

    // Shift register — reconstructs the byte as bits arrive.
    // After all 8 bits are shifted in, this holds the complete byte.
    signal shift_reg: bit[8]

    // Edge detection — store previous value of rx to detect
    // the falling edge that marks the start bit.
    signal rx_prev: bit

    // Output registers — we register these so the output is clean
    // and glitch-free.
    signal data_out:  bit[8]
    signal valid_out: bit

    on(clk.rise) {
        // Always capture previous rx for edge detection.
        // This runs every cycle regardless of state — rx_prev
        // is always one cycle behind rx.
        rx_prev = rx

        // Default: valid is only high for one cycle, so clear it
        // every cycle. The STOP state will set it to 1 on the
        // cycle a byte completes, and this default clears it on
        // the very next cycle.
        valid_out = 0

        if rst {
            state        = IDLE
            baud_counter = 0
            bit_index    = 0
            shift_reg    = 0
            rx_prev      = 1    // Line idles high
            data_out     = 0
            valid_out    = 0
        } else {
            match state {
                IDLE => {
                    // Reset counters while idle so they're ready
                    // when we detect a start bit.
                    baud_counter = 0
                    bit_index    = 0

                    // Detect falling edge: rx is low now, was high
                    // last cycle. This is the start bit.
                    if !rx && rx_prev {
                        state = START
                    }
                }

                START => {
                    // Count to the middle of the start bit.
                    // We detected the falling edge somewhere near the
                    // beginning of the start bit. By counting HALF_BIT
                    // (217) cycles, we reach the approximate centre.
                    if baud_counter == HALF_BIT {
                        baud_counter = 0

                        if !rx {
                            // Start bit confirmed at mid-point —
                            // the line is genuinely low, not just
                            // a noise glitch. Begin data reception.
                            state = DATA
                        } else {
                            // False start — the line bounced back
                            // high. This was noise, not a real
                            // start bit. Go back to idle.
                            state = IDLE
                        }
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }

                DATA => {
                    // Count a full bit period, then sample at mid-bit.
                    // After the start bit confirmation, we're at the
                    // centre of the start bit. From here, counting
                    // CYCLES_PER_BIT (434) cycles lands us at the
                    // centre of the next bit. Each subsequent count
                    // lands on the next bit's centre.
                    if baud_counter == CYCLES_PER_BIT {
                        baud_counter = 0

                        // Sample the rx line and shift into the register.
                        // UART sends LSB first, so we shift in from the top:
                        //   new bit goes into bit 7, existing bits shift right.
                        // After 8 samples:
                        //   - bit 0 = first bit transmitted (LSB)
                        //   - bit 7 = last bit transmitted (MSB)
                        shift_reg = (rx << 7) | (shift_reg >> 1)

                        if bit_index == 7 {
                            // All 8 bits received — move to stop bit
                            state = STOP
                        } else {
                            bit_index = bit_index + 1
                        }
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }

                STOP => {
                    // Wait for the centre of the stop bit. We could
                    // check that rx is high (valid stop bit), but for
                    // simplicity we just wait and output the byte
                    // regardless. A production design might flag a
                    // framing error if rx is low here.
                    if baud_counter == CYCLES_PER_BIT {
                        baud_counter = 0
                        state        = IDLE

                        // Latch the received byte and pulse valid
                        data_out  = shift_reg
                        valid_out = 1
                    } else {
                        baud_counter = baud_counter + 1
                    }
                }
            }
        }
    }

    // Drive output ports from registered values.
    // These combinational assignments wire the internal registers
    // to the entity's output ports.
    rx_data  = data_out
    rx_valid = valid_out
}
```

### How It Works

Let us walk through each of the key techniques in detail.

**Edge detection.** The `rx_prev` register holds the value of `rx` from the previous clock cycle. We update it unconditionally at the top of the `on` block — every single cycle, `rx_prev` gets the current value of `rx`. On the *next* cycle, `rx_prev` will hold what `rx` was, and `rx` will hold the new value. The expression `!rx && rx_prev` is true for exactly one cycle: the cycle where `rx` transitions from high to low. This is the falling edge that marks the beginning of a start bit.

This is the same technique the debouncer used with `btn_prev`. The only difference is what we do after detecting the edge: the debouncer starts a stability counter, while the UART receiver transitions to the START state.

**Mid-bit sampling.** When we detect the falling edge, we are somewhere within the first few nanoseconds of the start bit. The exact position depends on where the edge happened relative to our clock — it could be right at the clock edge, or up to one clock period (20 ns at 50 MHz) later. Either way, we are near the *beginning* of the start bit.

We count `HALF_BIT` (217) cycles to reach the *middle* of the start bit. At this point, we are 217 cycles into a 434-cycle bit period — right at the centre. We check that `rx` is still low to confirm this is a real start bit.

After confirmation, we count full `CYCLES_PER_BIT` (434) periods to reach the middle of each subsequent data bit. Because we started from the centre of the start bit, each 434-cycle count lands us at the centre of the next bit. This gives us maximum noise margin — we are sampling as far as possible from the transitions, where the signal is most stable.

**Bit reconstruction.** UART transmits LSB first. The first bit on the wire is bit 0 (the least significant), and the last is bit 7 (the most significant). We need to reassemble these into the correct byte.

The line `shift_reg = (rx << 7) | (shift_reg >> 1)` does this elegantly:

- `rx << 7` places the sampled bit at position 7 (the MSB of the register).
- `shift_reg >> 1` shifts all existing bits one position to the right.
- The OR combines them.

After the first sample, bit 7 holds D0. After the second sample, bit 7 holds D1 and bit 6 holds D0. After all eight samples, the bits are in the correct positions: D0 at bit 0, D1 at bit 1, and so on up to D7 at bit 7.

**One-cycle valid pulse.** The line `valid_out = 0` at the top of the `on` block runs every cycle as a default. The only place `valid_out` is set to 1 is inside the STOP state, on the cycle when `baud_counter` reaches `CYCLES_PER_BIT`. On the very next cycle, the default kicks in and clears it back to 0. This produces a clean single-cycle pulse that downstream logic can use to latch the data. This is a common RTL pattern: set a default at the top of the always block, then override it conditionally.

**Forward references.** Notice that the output port assignments `rx_data = data_out` and `rx_valid = valid_out` appear at the bottom, after the `on` block. These are combinational assignments that wire internal registers to output ports. In skalp, you can place these anywhere in the `impl` block — they could equally go at the top. The compiler resolves the dependency graph regardless of textual order. This lets you organise your code logically (sequential logic first, output wiring last) without fighting the language.

### Connecting TX and RX

With both `UartTx` (from Chapter 2) and `UartRx` in place, you can create a loopback test module that wires them together. This is a classic verification technique: connect the transmitter's output directly to the receiver's input and verify that bytes survive the round trip.

Create `src/uart_loopback.sk`:

```skalp
// uart_loopback.sk — connect TX output to RX input for testing

entity UartLoopback {
    in  clk:        clock
    in  rst:        reset
    in  send_data:  bit[8]
    in  send_en:    bit
    out recv_data:  bit[8]
    out recv_valid: bit
    out tx_busy:    bit
}

impl UartLoopback {
    // Internal wire connecting TX output to RX input.
    // In a real FPGA, these would be separate I/O pins.
    // In simulation, this direct connection gives us a
    // perfect channel with no noise or delay.
    signal serial_wire: bit

    let tx = UartTx {
        clk:     clk,
        rst:     rst,
        tx_data: send_data,
        tx_en:   send_en,
        tx:      serial_wire,
        tx_busy: tx_busy
    }

    let rx = UartRx {
        clk:      clk,
        rst:      rst,
        rx:       serial_wire,
        rx_data:  recv_data,
        rx_valid: recv_valid
    }
}
```

This wires the TX serial output directly to the RX serial input through `serial_wire`. The `let` bindings instantiate the two sub-modules and connect their ports. Notice how clean this is: each port connection is explicit, and the internal `serial_wire` signal provides the channel between the two modules without any external routing.

### Loopback Architecture

The loopback module is worth understanding as a design pattern. Here is what happens when you send a byte:

1. The testbench sets `send_data` to the desired byte and pulses `send_en`.
2. `UartTx` begins serialising: it pulls `serial_wire` low (start bit), then drives each data bit in sequence, then drives high (stop bit).
3. `UartRx` sees the falling edge on `serial_wire`, counts to mid-bit, and begins sampling.
4. After all 8 data bits and the stop bit, `UartRx` sets `recv_valid` high for one cycle and presents the byte on `recv_data`.
5. The testbench checks that `recv_data` matches `send_data`.

Because both modules use the same `CYCLES_PER_BIT` constant and share the same clock, the timing is perfect. In a real system, the TX and RX clocks might differ slightly, and the mid-bit sampling provides the tolerance needed to handle the mismatch. We will explore clock domain crossings in Chapter 8.

### Design Considerations

A few things this receiver does not handle, which a production design would:

- **Framing errors.** If the stop bit is low (rx = 0 when we expect the stop bit), the frame is corrupt. A production receiver would set a `frame_error` output.
- **Oversampling.** Professional UART receivers typically sample each bit 16 times (at 16x the baud rate) and take a majority vote. This provides much better noise immunity than our single mid-bit sample. The trade-off is a faster clock or a more complex counter.
- **Parity checking.** UART frames can include an optional parity bit between the data bits and the stop bit. Our design assumes 8N1 (8 data bits, no parity, 1 stop bit).
- **Break detection.** A "break" condition (rx held low for longer than a frame) can be used for signalling. A production receiver would detect and report this.

These features are straightforward to add using the patterns you have already learned. The FSM just needs more states and more condition checks.

---

## Build and Test

Compile the receiver on its own:

```bash
skalp build src/uart_rx.sk
```

Compile the loopback module (requires both TX and RX):

```bash
skalp build src/uart_loopback.sk
```

To run a quick simulation, you can use the skalp test runner. We will build a proper testbench in Chapter 10, but for now you can verify basic operation:

```bash
skalp test src/uart_loopback.sk --trace
```

This generates a VCD waveform file. Open it in GTKWave or your preferred viewer and check:

1. After asserting `send_en` with a data byte, `tx` (the `serial_wire`) goes low (start bit).
2. The `rx` module detects the falling edge and enters the START state.
3. `baud_counter` counts up to 217, then resets — the start bit is confirmed.
4. In the DATA state, `baud_counter` counts to 434 eight times, and `bit_index` increments from 0 to 7.
5. After each sample, `shift_reg` updates with the new bit shifted in from the top.
6. In the STOP state, `baud_counter` counts to 434 one last time.
7. `recv_valid` pulses high for exactly one cycle, and `recv_data` matches `send_data`.

Try sending several different byte values (0x00, 0xFF, 0xA5, 0x55) to verify the bit reconstruction works correctly for all bit patterns.

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Edge detection pattern | Store previous value, compare | `signal rx_prev: bit` then `!rx && rx_prev` |
| Mid-bit sampling | Count to half bit period | `if baud_counter == HALF_BIT { ... }` |
| Bit shifting (LSB first) | Shift right, insert at top | `(rx << 7) \| (shift_reg >> 1)` |
| One-cycle pulse | Default to 0, set to 1 conditionally | `valid_out = 0` at top of `on` block |
| Match in sequential block | `match` inside `on(clk.rise)` | `match state { IDLE => { ... }, ... }` |
| Loopback wiring | Internal signal connects two instances | `signal serial_wire: bit` |
| Instantiation with ports | `let name = Entity { ... }` | `let rx = UartRx { clk: clk, ... }` |
| Constants as signals | Named constant values | `signal HALF_BIT: nat[9] = 217` |

---

## Next

You now have both halves of the UART — a transmitter and a receiver. But there is an obvious problem: if the transmitter is busy sending one byte and the upstream logic wants to send another, the data is lost. Similarly, if the receiver completes a byte and the downstream logic is not ready to consume it, that byte vanishes.

What you need is a **buffer** between the application logic and the UART hardware — a place to queue up bytes waiting to be sent, and a place to hold received bytes until the application is ready to read them. This is a FIFO (first-in, first-out) queue.

Building a FIFO also gives us the perfect excuse to introduce two powerful skalp features: **array types** for the storage, and **generic parameters** so you can reuse the same FIFO at any width and depth.

In **[Chapter 4: Arrays and Generics](../04-arrays-and-generics/)**, you will build a **parameterized FIFO** using skalp's array types and generic parameters. You will then add TX and RX FIFOs to the UART so that bytes can be buffered in both directions, decoupling the producer and consumer from the serial timing.
