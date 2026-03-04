---
title: "Chapter 7: skalp Integration"
date: 2026-03-04
summary: "skalp pragmas for safety, CDC, and tracing. Formal verification of VHDL designs. Mixed skalp+VHDL projects. Get more from your VHDL with skalp-specific annotations."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 7
ShowToc: true
---

## What This Chapter Teaches

Up to this point, you have used skalp as a VHDL compiler and simulator -- a drop-in replacement for ModelSim or GHDL. But skalp is more than a build tool. It adds capabilities *on top* of standard VHDL: safety pragmas for redundancy, CDC annotations that catch metastability at compile time, tracing directives for debug visibility, and a formal verification engine that proves properties for all possible input sequences.

None of these features require non-standard VHDL. They are comment-based pragmas -- ignored by any other tool -- and skalp-specific commands that operate on your unchanged source.

By the end of this chapter you will understand:

- How the `-- skalp:` comment pragma system works
- How to annotate signals for triple modular redundancy, ECC, CDC synchronization, and debug tracing
- How to add formal verification assertions that skalp can prove exhaustively
- How to mix skalp-native `.sk` files with `.vhd` files in the same project
- How to instantiate VHDL entities from skalp code and vice versa
- How the UART design demonstrates these features in a realistic, non-trivial context

The reference design for this chapter is a full UART transmitter and receiver with baud rate generation, oversampled reception, and handshake signaling.

---

## The UART Design

Before diving into skalp-specific features, let us look at the design we will annotate. Create `src/uart.vhd`:

```vhdl
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;
use ieee.math_real.all;

entity uart is
    generic (
        baud                : positive := 9600;
        clock_frequency     : positive := 153600
    );
    port (
        clock               :   in  std_logic;
        reset               :   in  std_logic;
        data_stream_in      :   in  std_logic_vector(7 downto 0);
        data_stream_in_stb  :   in  std_logic;
        data_stream_in_ack  :   out std_logic;
        data_stream_out     :   out std_logic_vector(7 downto 0);
        data_stream_out_stb :   out std_logic;
        tx                  :   out std_logic;
        rx                  :   in  std_logic
    );
end uart;
```

This entity declares two generic parameters and a port list that implements a streaming data interface in both directions. Let us walk through the key architectural features.

### Generic Parameters

The `baud` and `clock_frequency` generics default to values that give clean integer division (153600 / 9600 = 16), but the instantiating design can override them. The type `positive` restricts both to values greater than zero -- a VHDL type constraint that skalp enforces at elaboration time.

### Compile-Time Math with `math_real`

Inside the architecture, the UART computes counter widths from the generic parameters:

```vhdl
constant c_tx_div       : integer := clock_frequency / baud;
constant c_tx_div_width : integer := integer(log2(real(c_tx_div))) + 1;
constant c_rx_div       : integer := clock_frequency / (baud * 16);
constant c_rx_div_width : integer := integer(log2(real(c_rx_div))) + 1;
```

The chain `integer(log2(real(c_tx_div))) + 1` converts the integer divisor to a real, takes the base-2 log from `ieee.math_real`, truncates back to integer, and adds one -- giving the minimum number of bits needed to represent the divisor. This is a standard VHDL idiom for computing signal widths from parameters. skalp evaluates these expressions at compile time; they produce constants, not hardware.

### Enumerated State Types

The UART uses two FSMs with named state types:

```vhdl
type uart_tx_states is (
    tx_send_start_bit, tx_send_data, tx_send_stop_bit
);
type uart_rx_states is (
    rx_get_start_bit, rx_get_data, rx_get_stop_bit
);
```

Each value is a named constant, not a magic number. skalp automatically chooses an efficient binary encoding during synthesis. In simulation, the state names appear in waveforms and debug output, making FSM behavior easy to trace.

### Oversampled Receiver

The receiver samples the `rx` line at 16 times the baud rate. A two-stage synchronizer captures the asynchronous input:

```vhdl
signal uart_rx_data_sr : std_logic_vector(1 downto 0) := (others => '1');
```

This shift register crosses the clock domain boundary -- the `rx` pin is asynchronous with respect to `clock`. The first flip-flop may go metastable; the second settles it. A digital filter then counts consecutive identical samples to reject glitches before the FSM acts on the data.

### Handshake Protocol

The TX path uses a strobe/acknowledge handshake: the upstream logic asserts `data_stream_in_stb` with valid data, the UART asserts `data_stream_in_ack` when it accepts the byte, and the upstream deasserts the strobe in response. The FSM only transitions from idle to sending when it sees the strobe.

### Named Processes

The architecture contains multiple named processes: `oversample_clock_divider` (16x baud tick), `rxd_synchronise` (two-stage synchronizer), `rxd_filter` (digital low-pass), `uart_receive_data` (RX FSM), and `uart_send_data` (TX FSM and baud generator). Named processes appear in skalp's diagnostic output, making it easy to locate issues.

---

## skalp Pragmas

skalp pragmas are VHDL comments that begin with `-- skalp:`. Any standard VHDL tool ignores them -- they are just comments. skalp recognizes them and acts on them during compilation, simulation, or formal analysis.

The general syntax is:

```vhdl
signal my_signal : std_logic; -- skalp:pragma_name
signal my_signal : std_logic; -- skalp:pragma_name(argument)
```

Pragmas can appear on signal declarations, process labels, or standalone comment lines. They are always on the same line as the construct they annotate, or on the line immediately preceding it.

### `-- skalp:trace` -- Debug Visibility

The `trace` pragma marks a signal for inclusion in debug output. When you run `skalp sim` with waveform capture, traced signals are always included even if the simulator would otherwise optimize them away. When you synthesize for an FPGA with `skalp build --target`, traced signals are connected to the on-chip logic analyzer.

Apply it to the UART state machines:

```vhdl
signal uart_tx_state : uart_tx_states := tx_send_start_bit; -- skalp:trace
signal uart_rx_state : uart_rx_states := rx_get_start_bit;  -- skalp:trace
```

Now `skalp sim --vcd` will always include `uart_tx_state` and `uart_rx_state` in the VCD file, and `skalp build --target ice40` will route these signals to the debug fabric.

You can trace any signal -- not just state machines. For high-fanout buses or deeply nested signals that are hard to find in a full waveform dump, `-- skalp:trace` is the simplest way to guarantee visibility.

### `-- skalp:cdc(sync)` -- Clock Domain Crossing

The `cdc` pragma tells skalp that a signal crosses a clock domain boundary. The argument specifies the synchronization strategy:

```vhdl
signal uart_rx_data_sr : std_logic_vector(1 downto 0) := (others => '1'); -- skalp:cdc(sync)
```

The `sync` argument means "this signal is synchronized by a multi-stage register chain." skalp verifies that the annotated signal feeds through at least two flip-flops before reaching combinational logic. If you accidentally read the first-stage output directly, skalp reports an error.

Other CDC annotations:

| Pragma | Meaning |
|--------|---------|
| `-- skalp:cdc(sync)` | Multi-flop synchronizer for single-bit signals |
| `-- skalp:cdc(handshake)` | Handshake-based crossing for multi-bit data |
| `-- skalp:cdc(gray)` | Gray-coded counter crossing (for FIFOs) |
| `-- skalp:cdc(async_fifo)` | Asynchronous FIFO boundary |

CDC bugs manifest as intermittent failures that depend on exact timing. By annotating crossing points, you give skalp enough information to check synchronization at compile time.

### `-- skalp:safety(tmr)` -- Triple Modular Redundancy

The `safety` pragma with the `tmr` argument tells skalp to triplicate a signal and add majority voting logic:

```vhdl
signal uart_tx_state : uart_tx_states := tx_send_start_bit; -- skalp:safety(tmr)
```

skalp creates three independent copies of `uart_tx_state` and a voter that selects the majority value. If a single-event upset (radiation hit) corrupts one copy, the other two outvote it. TMR is essential for space, aviation, and automotive applications. skalp handles the replication and voting automatically -- you do not need to manually triplicate your logic.

You can combine pragmas:

```vhdl
signal uart_tx_state : uart_tx_states := tx_send_start_bit; -- skalp:safety(tmr) skalp:trace
```

### `-- skalp:safety(ecc)` -- Error Correction Coding

For data registers where full triplication is too expensive, ECC provides single-error correction and double-error detection with lower overhead:

```vhdl
signal tx_data : std_logic_vector(7 downto 0); -- skalp:safety(ecc)
```

skalp adds parity bits to the stored value and inserts correction logic on every read. A single bit flip is corrected silently; a double bit flip is detected and flagged. The overhead is proportional to `log2(width)` additional bits, not 3x the full signal width.

### `-- skalp:breakpoint` -- Simulation Breakpoints

The `breakpoint` pragma halts the simulator when a condition is reached:

```vhdl
-- skalp:breakpoint
assert uart_tx_state /= tx_send_start_bit or data_stream_in_stb = '0'
    report "TX started unexpectedly" severity note;
```

During `skalp sim`, when the assertion condition fails, the simulator pauses and drops into the skalp debugger (or halts if non-interactive). You can inspect signal values, step forward, or resume.

### Summary of Pragmas

| Pragma | Target | Effect |
|--------|--------|--------|
| `-- skalp:trace` | Signal | Include in debug/waveform output |
| `-- skalp:cdc(sync)` | Signal | Verify synchronizer chain |
| `-- skalp:cdc(handshake)` | Signal | Verify handshake crossing |
| `-- skalp:cdc(gray)` | Signal | Verify gray-coded crossing |
| `-- skalp:cdc(async_fifo)` | Signal | Verify async FIFO boundary |
| `-- skalp:safety(tmr)` | Signal | Triplicate with majority voter |
| `-- skalp:safety(ecc)` | Signal | Add error correction coding |
| `-- skalp:breakpoint` | Assertion | Halt simulation on condition |
| `-- skalp:formal` | Assertion | Include in formal verification |

---

## Formal Verification

Simulation tests your design against specific input sequences. Formal verification proves properties for *all possible* input sequences. If a formal property holds, it holds for every reachable state of the design -- no corner case can slip through.

skalp converts VHDL assertions annotated with `-- skalp:formal` into formal properties and feeds them to its built-in model checker. You do not need to learn PSL or SVA -- standard VHDL `assert` statements are sufficient.

### Adding Formal Properties to the UART

The TX path should never accept a strobe while it is busy transmitting. Express this as an assertion:

```vhdl
-- skalp:formal
assert not (data_stream_in_stb = '1' and data_stream_in_ack = '0'
            and uart_tx_state /= tx_send_start_bit)
    report "Data strobe asserted while TX is busy" severity failure;
```

If the upstream logic violates the handshake protocol by asserting the strobe while the UART is busy, this property will fail.

Another useful property -- the TX line should always be high when idle:

```vhdl
-- skalp:formal
assert uart_tx_state /= tx_send_start_bit or tx = '1'
    report "TX line not idle-high when FSM is idle" severity failure;
```

And a liveness property -- the TX FSM should always eventually return to idle:

```vhdl
-- skalp:formal
-- skalp:formal_type(liveness)
assert uart_tx_state = tx_send_start_bit
    report "TX FSM stuck in non-idle state" severity failure;
```

The `liveness` annotation tells skalp this is not a safety property (which must hold at every cycle) but a liveness property (which must hold *eventually*). skalp uses different proof strategies for each.

### Running Formal Verification

```bash
skalp verify --entity uart
```

Output:

```
   Verifying uart
   [1/3] TX handshake safety .............. PROVED (depth 24)
   [2/3] TX idle-high .................... PROVED (depth 16)
   [3/3] TX FSM liveness ................. PROVED (depth 48)
   All 3 properties verified.
```

The `depth` indicates how many clock cycles the model checker explored. The result is exhaustive -- not a bounded check.

### When Formal Finds a Bug

If a property fails, skalp generates a counterexample:

```
   [1/3] TX handshake safety .............. FAILED
   Counterexample (12 cycles):
     cycle  0: reset=1
     cycle  1: reset=0
     cycle  2: data_stream_in_stb=1, data_stream_in=0x41
     cycle  3: data_stream_in_ack=1
     cycle  4: data_stream_in_stb=1, data_stream_in=0x42  <-- VIOLATION
   Property violated: Data strobe asserted while TX is busy
```

You can replay the counterexample with `skalp sim --replay` to see the full waveform.

### Formal vs Simulation

| Aspect | Simulation | Formal Verification |
|--------|-----------|-------------------|
| Coverage | Tests specific input sequences you write | Proves for all possible inputs |
| Speed | Fast per test, but cannot cover all states | Slower, but exhaustive |
| Bugs found | Only bugs triggered by your test vectors | Any reachable bug |
| Setup | Write testbench code | Write assertions in the design |
| Best for | Functional testing, integration | Protocol correctness, invariants |

Use both. Simulation tests that the design *does what you want*. Formal verification proves that it *never does what you forbid*.

---

## Annotating the Full UART

Here is the UART's signal declaration section with skalp pragmas applied:

```vhdl
    -- TX signals
    signal uart_tx_state   : uart_tx_states := tx_send_start_bit; -- skalp:trace skalp:safety(tmr)
    signal uart_tx_data    : std_logic_vector(7 downto 0);        -- skalp:safety(ecc)
    signal uart_tx_count   : unsigned(c_tx_div_width-1 downto 0);
    signal uart_tx_bit     : unsigned(2 downto 0);

    -- RX synchronizer and filter
    signal uart_rx_data_sr : std_logic_vector(1 downto 0) := (others => '1'); -- skalp:cdc(sync)
    signal uart_rx_filter  : unsigned(1 downto 0);
    signal uart_rx_bit     : std_logic := '1';

    -- RX state
    signal uart_rx_state   : uart_rx_states := rx_get_start_bit;  -- skalp:trace skalp:safety(tmr)
    signal uart_rx_data    : std_logic_vector(7 downto 0);
    signal uart_rx_count   : unsigned(c_rx_div_width-1 downto 0);
    signal uart_rx_bit_cnt : unsigned(2 downto 0);
```

The choices are deliberate: **state registers get TMR and trace** (a bit flip in the FSM state is catastrophic, and you always want state visibility in debug). **TX data gets ECC** (triplicating 8 bits costs 16 extra flip-flops; ECC costs only 4 parity bits). **The RX synchronizer gets CDC** (skalp verifies the two-stage chain is intact). **Counters are left unannotated** (a bit flip self-corrects on the next counter reload).

---

## Mixed skalp+VHDL Projects

skalp supports projects that contain both `.sk` (skalp-native) and `.vhd` (VHDL) files. This lets you write new logic in skalp while reusing existing VHDL IP, or wrap a VHDL design with a skalp-native top level.

### Project Configuration

```toml
[package]
name = "mixed-project"
version = "0.1.0"

[build]
top = "SystemTop"
sources = ["src/*.sk", "src/*.vhd"]
```

The `sources` array lists glob patterns for both file types. skalp compiles each with the appropriate frontend and links them into a unified design hierarchy.

### Instantiating VHDL from skalp

Suppose `src/uart.vhd` defines the `uart` entity shown above, and you want to instantiate it from `src/top.sk`:

```
entity SystemTop {
    clock      : in  bit,
    reset      : in  bit,
    uart_tx    : out bit,
    uart_rx    : in  bit,
    led        : out bit[8],
}

impl SystemTop {
    let tx_data  : bit[8];
    let tx_stb   : bit;
    let tx_ack   : bit;
    let rx_data  : bit[8];
    let rx_stb   : bit;

    inst my_uart = uart {
        clock           = clock,
        reset           = reset,
        data_stream_in  = tx_data,
        data_stream_in_stb = tx_stb,
        data_stream_in_ack = tx_ack,
        data_stream_out = rx_data,
        data_stream_out_stb = rx_stb,
        tx              = uart_tx,
        rx              = uart_rx,
    } with {
        baud            = 115200,
        clock_frequency = 50_000_000,
    };

    // Echo received data back, display on LEDs
    tx_data = rx_data;
    tx_stb  = rx_stb;
    led     = rx_data;
}
```

The `inst` keyword instantiates the VHDL entity by name. skalp resolves entities across language boundaries. Port connections use `=` (skalp syntax), and generics are passed in the `with` block.

### Instantiating skalp from VHDL

Going the other direction, suppose `src/blinker.sk` defines a skalp entity:

```
entity Blinker {
    clk    : in  bit,
    rst    : in  bit,
    led    : out bit,
} with {
    period : nat[32] = 50_000_000,
}
```

You can instantiate it from VHDL using direct entity instantiation:

```vhdl
architecture rtl of top_board is
begin
    blink_inst : entity work.Blinker
        generic map (
            period => 25_000_000
        )
        port map (
            clk => clock,
            rst => reset,
            led => heartbeat_led
        );
end architecture rtl;
```

skalp compiles the entity into a VHDL-compatible interface, so the instantiation syntax is standard. The entity name is case-sensitive. Generics map to `generic map`, ports to `port map`.

### Type Mapping Between Languages

When crossing the language boundary, skalp maps types automatically:

| skalp Type | VHDL Type |
|-----------|-----------|
| `bit` | `std_logic` |
| `bit[N]` | `std_logic_vector(N-1 downto 0)` |
| `nat[N]` | `unsigned(N-1 downto 0)` |
| `int[N]` | `signed(N-1 downto 0)` |
| `bool` | `std_logic` ('1' for true, '0' for false) |

Port directions (`in`, `out`, `inout`) map directly between the two languages.

---

> **Coming from skalp?**
>
> If you write skalp-native code, the features described in this chapter are already part of the language:
>
> | VHDL + pragma | skalp native | Notes |
> |--------------|-------------|-------|
> | `-- skalp:safety(tmr)` | `@safety(tmr) signal state` | First-class annotation, not a comment |
> | `-- skalp:cdc(sync)` | `@cdc(sync) signal rx_sync` | Type system tracks clock domains |
> | `-- skalp:trace` | `@trace signal debug_sig` | Same effect, different syntax |
> | `-- skalp:formal` + `assert` | `formal { assert ... }` | Dedicated formal block |
> | `-- skalp:breakpoint` | `@breakpoint` | Attribute syntax |
>
> The key difference: in skalp, CDC safety is enforced by the type system. A signal in clock domain A *cannot* be read from domain B without a synchronizer -- the compiler rejects it. In VHDL, skalp can only check what you annotate. The VHDL pragma approach is opt-in and backward compatible; the skalp-native approach is mandatory and catches more bugs.

---

## Testing the UART

Create `tests/uart_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_uart_tx() {
    let mut tb = Testbench::new("src/uart.vhd").await.unwrap();
    tb.reset(2).await;

    // Send a byte (0x55 = alternating bits, good for baud rate testing)
    tb.set("data_stream_in", 0x55u32);
    tb.set("data_stream_in_stb", 1u8);

    // Wait for acknowledgment
    for _ in 0..100 {
        tb.clock(1).await;
        if tb.get_u64("data_stream_in_ack").await == 1 {
            break;
        }
    }
    tb.expect("data_stream_in_ack", 1u32).await;
    tb.set("data_stream_in_stb", 0u8);

    // Wait for transmission to complete
    tb.clock(200).await;
}
```

The test resets, presents a byte with the strobe, polls for acknowledgment (the loop avoids hardcoding cycle counts), deasserts the strobe, and waits for the transmission to complete. With default generics (153600 Hz clock, 9600 baud), one byte takes 16 cycles/bit times 10 bits = 160 cycles.

### Testing the RX Path

```rust
#[tokio::test]
async fn test_uart_rx() {
    let mut tb = Testbench::new("src/uart.vhd").await.unwrap();
    tb.reset(2).await;

    // The rx line idles high
    tb.set("rx", 1u8);
    tb.clock(10).await;

    // Send start bit (low)
    tb.set("rx", 0u8);
    tb.clock(16).await; // One bit period at 16x oversample

    // Send 0x55 = 01010101, LSB first
    let byte: u8 = 0x55;
    for i in 0..8 {
        let bit = (byte >> i) & 1;
        tb.set("rx", bit);
        tb.clock(16).await;
    }

    // Send stop bit (high)
    tb.set("rx", 1u8);
    tb.clock(16).await;

    // Check that the received byte is available
    tb.clock(10).await; // Allow pipeline to settle
    tb.expect("data_stream_out_stb", 1u32).await;
    tb.expect("data_stream_out", 0x55u32).await;
}
```

This test manually constructs a UART frame on the `rx` pin: idle high, start bit (16 oversample clocks), 8 data bits LSB-first (0x55), and a stop bit. It then verifies that `data_stream_out_stb` is asserted and `data_stream_out` contains the correct byte.

Combine simulation tests with formal verification: `cargo test` verifies the UART transmits and receives specific bytes, while `skalp verify --entity uart` proves the handshake invariants hold for all possible input sequences.

---

## Quick Reference

| Feature | Syntax | Command |
|---------|--------|---------|
| Trace a signal | `-- skalp:trace` | Visible in `skalp sim --vcd` output |
| CDC annotation | `-- skalp:cdc(sync)` | Checked during `skalp build` |
| TMR protection | `-- skalp:safety(tmr)` | Applied during `skalp build` |
| ECC protection | `-- skalp:safety(ecc)` | Applied during `skalp build` |
| Simulation breakpoint | `-- skalp:breakpoint` | Triggers during `skalp sim` |
| Formal assertion | `-- skalp:formal` | Checked by `skalp verify` |
| Formal liveness | `-- skalp:formal_type(liveness)` | Checked by `skalp verify` |
| Mixed sources | `sources = ["src/*.sk", "src/*.vhd"]` | In `skalp.toml` `[build]` section |
| Instantiate VHDL from skalp | `inst name = entity { ... }` | Port map + generic `with` block |
| Instantiate skalp from VHDL | `entity work.Name port map (...)` | Standard VHDL direct instantiation |
| Run formal | `skalp verify --entity name` | Proves all `-- skalp:formal` assertions |
| Replay counterexample | `skalp sim --replay` | After a failed `skalp verify` |

---

**Exercise:** Add `-- skalp:trace` to the baud rate counters. Run `skalp sim --vcd` and verify in GTKWave that the TX counter period matches `clock_frequency / baud`. Then write a formal property asserting the TX line is never low for more than 10 bit periods (hint: you need an auxiliary counter).

---

## Next: VHDL-2019 Features

The pragmas and formal verification in this chapter work with any VHDL standard from VHDL-93 onward. But VHDL has continued to evolve, and the 2019 revision introduces features that most free tools do not support: interfaces, views, and generic type parameters.

In Chapter 8, you will learn:

- How VHDL-2019 interfaces bundle related signals into a single named connection
- How views define which signals are inputs and outputs from each side of an interface
- How generic type parameters let you write truly reusable components
- Why skalp is one of the few free tools that compiles VHDL-2019

Continue to [Chapter 8: VHDL-2019 Features](../08-vhdl-2019/).
