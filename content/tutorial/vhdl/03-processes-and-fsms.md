---
title: "Chapter 3: Clocked Processes and State Machines"
date: 2026-03-04
summary: "Enumerated state types, FSM patterns with case statements, prescaler-based timing, and type casting — build a timer and an I2C-style controller, both compiled with skalp."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 3
ShowToc: true
---

## What This Chapter Teaches

Chapters 1 and 2 covered the basics: a counter with a single clocked process and combinational logic with `process(all)`. Real designs need more. This chapter introduces four concepts you will use in nearly every VHDL module:

- **Multiple clocked processes** in one architecture, each running concurrently
- **Enumerated types** for state machines: `type state_t is (ST_IDLE, ST_START, ...)`
- **FSM patterns** with `case state is` inside a clocked process
- **Type casting** between `std_logic_vector`, `unsigned`, and `std_logic`

We build two designs:

1. **A configurable timer** with a prescaler and threshold comparator -- two processes, heavy type casting, hex literals.
2. **An I2C-style FSM controller** -- enumerated states, bit shifting with concatenation, `with...select` for concurrent output decoding.

Both designs compile, simulate, and pass testbenches under skalp.

---

## Design 1: Configurable Timer

The timer counts clock cycles with a configurable prescaler divider. When the count reaches a threshold, it pulses `match_out`. If the counter overflows past `0xFF`, it pulses `overflow` instead.

### Entity

```vhdl
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity timer is
    port (
        clk       : in  std_logic;
        rst       : in  std_logic;
        enable    : in  std_logic;
        prescaler : in  std_logic_vector(3 downto 0);
        threshold : in  std_logic_vector(7 downto 0);
        counter   : out std_logic_vector(7 downto 0);
        match_out : out std_logic;
        overflow  : out std_logic
    );
end entity timer;
```

The ports use `std_logic_vector` -- the standard bus type for interfaces. Internally, we will use `unsigned` for arithmetic and cast at the boundaries.

### Architecture: Two Concurrent Processes

```vhdl
architecture rtl of timer is
    signal cnt_reg     : unsigned(7 downto 0);
    signal prescale_cnt: unsigned(15 downto 0);
    signal tick        : std_logic;
    signal match_flag  : std_logic;
begin
```

The architecture declares four internal signals. `cnt_reg` is the main counter; `prescale_cnt` is a free-running divider; `tick` is the divided clock enable; `match_flag` holds the threshold match for one cycle.

Two processes follow. Each is a separate concurrent block -- they do not execute in sequence. The simulator evaluates both on every rising clock edge.

### Process 1: Prescaler

```vhdl
    prescale_proc: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                prescale_cnt <= (others => '0');
                tick <= '0';
            elsif enable = '1' then
                prescale_cnt <= prescale_cnt + 1;
                case prescaler is
                    when "0000" => tick <= '1';
                    when "0001" => tick <= std_logic(prescale_cnt(0));
                    when "0010" => tick <= std_logic(prescale_cnt(1));
                    when "0011" => tick <= std_logic(prescale_cnt(2));
                    when "0100" => tick <= std_logic(prescale_cnt(3));
                    when others => tick <= std_logic(prescale_cnt(4));
                end case;
            else
                tick <= '0';
            end if;
        end if;
    end process prescale_proc;
```

**Named processes.** The label `prescale_proc:` before `process` names the block. This is optional but strongly recommended -- it appears in waveform viewers and simulation traces, making debugging far easier.

**Prescaler logic.** The `prescale_cnt` register increments every enabled clock cycle. The `case prescaler is` selects which bit of `prescale_cnt` drives `tick`. When `prescaler = "0000"`, tick is always `'1'` (no division). When `prescaler = "0001"`, tick follows bit 0, which toggles every cycle (divide by 2). Each higher value doubles the division ratio.

**Type casting: `std_logic(prescale_cnt(0))`.**  The expression `prescale_cnt(0)` returns a value of type `unsigned` (a single-element slice). The cast `std_logic(...)` converts it to `std_logic` so it can be assigned to `tick`. This is one of the most common type casts in VHDL. The rule: `unsigned` and `std_logic_vector` can be cast to each other freely because they are closely related types defined in `ieee.numeric_std`.

### Process 2: Counter

```vhdl
    count_proc: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                cnt_reg    <= (others => '0');
                match_flag <= '0';
                overflow   <= '0';
            elsif tick = '1' then
                if cnt_reg = X"FF" then
                    cnt_reg    <= (others => '0');
                    match_flag <= '0';
                    overflow   <= '1';
                elsif cnt_reg = unsigned(threshold) then
                    cnt_reg    <= (others => '0');
                    match_flag <= '1';
                    overflow   <= '0';
                else
                    cnt_reg    <= cnt_reg + 1;
                    match_flag <= '0';
                    overflow   <= '0';
                end if;
            else
                match_flag <= '0';
                overflow   <= '0';
            end if;
        end if;
    end process count_proc;
```

**`unsigned(threshold)`.**  The port `threshold` is `std_logic_vector(7 downto 0)`. The counter `cnt_reg` is `unsigned(7 downto 0)`. VHDL does not allow direct comparison between these types. The cast `unsigned(threshold)` converts the vector to unsigned so the `=` comparison works.

**Hex literals: `X"FF"`.** VHDL hex literals use the `X"..."` syntax. `X"FF"` is an 8-bit value, all ones -- the maximum for an unsigned byte. The width is inferred from context (the comparison with `cnt_reg`).

**Multiple conditions.** The `if/elsif/else` chain inside `tick = '1'` checks three cases: overflow, threshold match, and normal increment. The overflow check comes first so that when the counter reaches `X"FF"`, it overflows regardless of the threshold value. Each branch explicitly assigns all three outputs (`cnt_reg`, `match_flag`, `overflow`) to avoid inferred latches.

### Output Assignments

```vhdl
    counter   <= std_logic_vector(cnt_reg);
    match_out <= match_flag;
end architecture rtl;
```

These concurrent assignments sit outside both processes. `std_logic_vector(cnt_reg)` casts the internal `unsigned` back to `std_logic_vector` for the output port. This is the mirror of `unsigned(threshold)` above -- casting at the boundary, computing with `unsigned` internally.

### Type Casting Summary

The timer uses three casts. Here they are in one place:

| Expression | From | To | Why |
|---|---|---|---|
| `unsigned(threshold)` | `std_logic_vector` | `unsigned` | Compare with `cnt_reg` |
| `std_logic(prescale_cnt(0))` | `unsigned` element | `std_logic` | Assign to `tick` |
| `std_logic_vector(cnt_reg)` | `unsigned` | `std_logic_vector` | Drive output port |

The general pattern: use `std_logic_vector` on ports, `unsigned` (or `signed`) inside for arithmetic, and cast at the boundaries.

---

## Design 2: I2C-Style FSM Controller

The second design is an I2C bus controller that demonstrates the classic VHDL FSM pattern. It handles start conditions, data shifting, acknowledgment, and stop conditions.

### Entity

```vhdl
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity i2c_fsm is
    port (
        clk      : in  std_logic;
        rst      : in  std_logic;
        start    : in  std_logic;
        stop     : in  std_logic;
        wr_data  : in  std_logic_vector(7 downto 0);
        rd_data  : out std_logic_vector(7 downto 0);
        busy     : out std_logic;
        done     : out std_logic;
        scl_out  : out std_logic;
        sda_out  : out std_logic;
        sda_in   : in  std_logic
    );
end entity i2c_fsm;
```

The entity has control signals (`start`, `stop`, `busy`, `done`), a data path (`wr_data`, `rd_data`), and the I2C physical lines (`scl_out`, `sda_out`, `sda_in`).

### Enumerated State Type

```vhdl
architecture rtl of i2c_fsm is
    type state_t is (ST_IDLE, ST_START, ST_DATA, ST_ACK, ST_STOP, ST_DONE);
    signal state     : state_t;
    signal bit_cnt   : unsigned(3 downto 0);
    signal shift_reg : std_logic_vector(7 downto 0);
    signal rx_reg    : std_logic_vector(7 downto 0);
    signal clk_div   : unsigned(3 downto 0);
    signal scl_en    : std_logic;
    signal phase     : std_logic_vector(1 downto 0);
begin
```

**`type state_t is (ST_IDLE, ST_START, ST_DATA, ST_ACK, ST_STOP, ST_DONE);`** -- This declares an enumerated type with six named values. The signal `state` uses this type. In simulation and waveform viewers, you see the symbolic names instead of raw bit values, which makes debugging much simpler.

Enumerated types are the standard way to write FSMs in VHDL. The synthesizer maps each value to a binary encoding (one-hot, binary, or gray code depending on tool settings). You write symbolic names; the tool picks the encoding.

### Clock Divider Process

```vhdl
    clk_gen: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                clk_div <= (others => '0');
                phase   <= "00";
            else
                clk_div <= clk_div + 1;
                if clk_div = "1111" then
                    phase <= std_logic_vector(unsigned(phase) + 1);
                end if;
            end if;
        end if;
    end process clk_gen;
```

The `clk_div` counter divides the system clock. When it reaches `"1111"` (15), `phase` advances. The expression `std_logic_vector(unsigned(phase) + 1)` is a double cast: convert `phase` from `std_logic_vector` to `unsigned` for arithmetic, add 1, then cast back. This is verbose but explicit -- VHDL does not allow arithmetic on `std_logic_vector` directly.

### Concurrent Output with `with...select`

```vhdl
    with state select
        scl_out <= '1'        when ST_IDLE,
                   '0'        when ST_START,
                   phase(1)   when ST_DATA,
                   phase(1)   when ST_ACK,
                   '1'        when ST_STOP,
                   '1'        when others;
```

**`with...select`** is a concurrent signal assignment -- it lives outside any process. It works like a multiplexer: the value of `state` selects which expression drives `scl_out`. This is equivalent to a `case` inside a combinational process, but more compact for single-signal output decoding.

The `when others` clause is required. Even though all six states are listed, VHDL insists on a default to handle any encoding the synthesizer might generate.

### The FSM Process

```vhdl
    fsm: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                state     <= ST_IDLE;
                bit_cnt   <= (others => '0');
                shift_reg <= (others => '0');
                rx_reg    <= (others => '0');
                sda_out   <= '1';
                busy      <= '0';
                done      <= '0';
            else
                done <= '0';
                case state is
                    when ST_IDLE =>
                        sda_out <= '1';
                        busy    <= '0';
                        if start = '1' then
                            state     <= ST_START;
                            shift_reg <= wr_data;
                            busy      <= '1';
                        end if;

                    when ST_START =>
                        sda_out <= '0';
                        bit_cnt <= to_unsigned(7, 4);
                        state   <= ST_DATA;

                    when ST_DATA =>
                        sda_out <= shift_reg(7);
                        if phase = "11" and clk_div = "1111" then
                            rx_reg    <= rx_reg(6 downto 0) & sda_in;
                            shift_reg <= shift_reg(6 downto 0) & '0';
                            if bit_cnt = "0000" then
                                state <= ST_ACK;
                            else
                                bit_cnt <= bit_cnt - 1;
                            end if;
                        end if;

                    when ST_ACK =>
                        sda_out <= '1';
                        if phase = "11" and clk_div = "1111" then
                            if stop = '1' then
                                state <= ST_STOP;
                            else
                                state <= ST_DONE;
                            end if;
                        end if;

                    when ST_STOP =>
                        sda_out <= '0';
                        if phase = "10" then
                            sda_out <= '1';
                            state   <= ST_DONE;
                        end if;

                    when ST_DONE =>
                        done  <= '1';
                        busy  <= '0';
                        state <= ST_IDLE;

                    when others =>
                        state <= ST_IDLE;
                end case;
            end if;
        end if;
    end process fsm;

    rd_data <= rx_reg;
end architecture rtl;
```

This is the core FSM. Let us walk through the key patterns.

**`case state is` / `when ST_IDLE =>`.**  The case statement dispatches on the enumerated state signal. Each `when` branch handles one state and assigns next-state logic plus output values. This is the canonical VHDL FSM structure.

**`to_unsigned(7, 4)`.**  The function `to_unsigned(value, width)` creates an unsigned literal with an explicit bit width. Here, 7 as a 4-bit unsigned (`"0111"`). This is needed because VHDL integer literals have no inherent width -- you must tell the compiler how wide the result should be.

**Concatenation: `rx_reg(6 downto 0) & sda_in`.**  The `&` operator concatenates bit vectors. `rx_reg(6 downto 0)` takes the lower 7 bits; `& sda_in` appends one bit on the right. The result is an 8-bit vector with the new bit shifted in at the LSB. This is how you build a shift register in VHDL.

**Shift out: `shift_reg(6 downto 0) & '0'`.**  Same pattern in reverse -- the MSB is dropped, a zero enters from the right. The current MSB (`shift_reg(7)`) drives `sda_out` before the shift happens.

**Default assignment: `done <= '0';`.**  This line sits above the `case` block, before any state branch runs. It provides a default value for `done` that any branch can override. In `ST_DONE`, the branch sets `done <= '1'`, which overrides the default. This avoids repeating `done <= '0'` in every other state.

---

## Coming from skalp?

If you have written skalp designs and are reading VHDL for the first time, here are the key equivalences.

**Enumerated types.** VHDL `type state_t is (ST_IDLE, ST_START, ...)` maps directly to skalp enums (covered in Chapter 7 of the skalp tutorial):

```
// skalp
enum State { Idle, Start, Data, Ack, Stop, Done }
```

```vhdl
-- VHDL
type state_t is (ST_IDLE, ST_START, ST_DATA, ST_ACK, ST_STOP, ST_DONE);
```

**State dispatch.** In skalp, you match on enum values or use `if state == State::Idle`. In VHDL, the equivalent is `case state is` / `when ST_IDLE =>`.

**Type casting.** skalp infers widths and allows implicit conversions in many contexts. VHDL requires explicit casts: `unsigned(threshold)`, `std_logic_vector(cnt_reg)`. This verbosity is intentional -- VHDL is strongly typed to catch width mismatches at compile time.

**Multiple processes.** In skalp, a single `always` block describes the sequential behavior. In VHDL, an architecture can contain multiple named processes that all run concurrently. Each process is like a separate `always` block.

---

## Build and Test

Save the timer as `src/timer.vhd` and the I2C controller as `src/i2c_fsm.vhd` in your skalp project.

### Compile

```bash
skalp build src/timer.vhd
skalp build src/i2c_fsm.vhd
```

Both should compile without errors. If you see type mismatch warnings, check that you have the `use ieee.numeric_std.all;` import -- without it, the casts and arithmetic operators are not in scope.

### Testbench 1: Timer

Create `tests/timer_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_timer_counts_without_prescaler() {
    let mut tb = Testbench::new("src/timer.vhd").await.unwrap();

    // Reset
    tb.set("rst", 1u8);
    tb.set("enable", 0u8);
    tb.set("prescaler", 0b0000u8);   // No prescaler division
    tb.set("threshold", 0x0Au8);     // Match at 10
    tb.clock(2).await;

    // Release reset, enable counting
    tb.set("rst", 0u8);
    tb.set("enable", 1u8);

    // Clock 12 cycles -- 1 for prescaler startup, 10 counting, 1 match trigger
    tb.clock(12).await;
    tb.expect("match_out", 1u8).await;
    tb.expect("counter", 0u8).await;   // Resets to 0 on match
}

#[tokio::test]
async fn test_timer_overflow() {
    let mut tb = Testbench::new("src/timer.vhd").await.unwrap();

    tb.set("rst", 1u8);
    tb.set("enable", 0u8);
    tb.set("prescaler", 0b0000u8);
    tb.set("threshold", 0xFFu8);     // Threshold at max -- will never match before overflow
    tb.clock(2).await;

    tb.set("rst", 0u8);
    tb.set("enable", 1u8);

    // Clock 257 cycles -- 1 prescaler startup + 255 counting + 1 overflow trigger
    tb.clock(257).await;
    tb.expect("overflow", 1u8).await;
}
```

Run with:

```bash
cargo test --test timer_test
```

The first test sets `prescaler` to `"0000"` (tick every cycle) and `threshold` to 10. After 12 clocks (1 cycle for the prescaler to produce the first tick, 10 cycles of counting, and 1 cycle for the match to trigger), `match_out` should pulse and the counter should reset. The second test sets the threshold to the maximum and clocks through 257 cycles to trigger overflow -- since the overflow check has priority over the threshold match, the counter overflows at `0xFF` instead of matching.

### Testbench 2: I2C FSM

Create `tests/i2c_fsm_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_i2c_start_transfer() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();

    // Reset
    tb.set("rst", 1u8);
    tb.set("start", 0u8);
    tb.set("stop", 0u8);
    tb.set("wr_data", 0xA5u8);
    tb.set("sda_in", 1u8);
    tb.clock(2).await;

    // Release reset
    tb.set("rst", 0u8);
    tb.clock(1).await;

    // Should be idle
    tb.expect("busy", 0u8).await;
    tb.expect("done", 0u8).await;

    // Start a transfer
    tb.set("start", 1u8);
    tb.clock(1).await;
    tb.set("start", 0u8);

    // Should be busy now
    tb.expect("busy", 1u8).await;
}

#[tokio::test]
async fn test_i2c_completes_transfer() {
    let mut tb = Testbench::new("src/i2c_fsm.vhd").await.unwrap();

    // Reset and start
    tb.set("rst", 1u8);
    tb.set("start", 0u8);
    tb.set("stop", 1u8);           // Request stop after data
    tb.set("wr_data", 0x55u8);
    tb.set("sda_in", 0u8);         // ACK = 0
    tb.clock(2).await;

    tb.set("rst", 0u8);
    tb.clock(1).await;

    // Trigger transfer
    tb.set("start", 1u8);
    tb.clock(1).await;
    tb.set("start", 0u8);

    // Poll for completion -- done is a single-cycle pulse,
    // so we must check every cycle
    for _ in 0..1000 {
        tb.clock(1).await;
        if tb.get_u64("done").await == 1 {
            break;
        }
    }
    tb.expect("done", 1u8).await;
    tb.expect("busy", 0u8).await;
}
```

Run with:

```bash
cargo test --test i2c_fsm_test
```

The first test verifies that asserting `start` transitions the FSM out of idle and sets `busy` high. The second test runs a complete transfer cycle -- start, 8 data bits, ACK, stop -- and polls for the `done` signal. Because `done` is a single-cycle pulse (the FSM returns to idle on the very next clock), the test must check every cycle rather than clocking a fixed number of cycles and then checking.

To capture waveforms for debugging, add an `export_waveform` call to your test:

```rust
tb.export_waveform("build/i2c_fsm.skw.gz").unwrap();
```

Open the `.skw.gz` file in the skalp VS Code extension. The `state` signal will display symbolic names (`ST_IDLE`, `ST_DATA`, etc.).

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Named process | `label: process(clk)` | `prescale_proc: process(clk)` |
| Enumerated type | `type name is (val, ...)` | `type state_t is (ST_IDLE, ST_START, ...)` |
| Case on enum | `case sig is when val => ...` | `case state is when ST_IDLE => ...` |
| Cast to unsigned | `unsigned(slv_signal)` | `unsigned(threshold)` |
| Cast to std_logic_vector | `std_logic_vector(uns_signal)` | `std_logic_vector(cnt_reg)` |
| Cast to std_logic | `std_logic(uns_element)` | `std_logic(prescale_cnt(0))` |
| Create unsigned literal | `to_unsigned(val, width)` | `to_unsigned(7, 4)` |
| Hex literal | `X"hex_digits"` | `X"FF"` |
| Concatenation | `a & b` | `shift_reg(6 downto 0) & '0'` |
| Concurrent select | `with sig select out <= ...` | `with state select scl_out <= ...` |
| Aggregate reset | `(others => '0')` | `cnt_reg <= (others => '0')` |

---

## What is Next

[Chapter 4: Generics, Records, and Arrays](../04-generics-and-types/) introduces parameterized designs with `generic`, user-defined record types for grouping related signals, and array types for register banks. You will build a GPIO controller whose width is set by a generic parameter.
