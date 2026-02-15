---
title: "GPU-Accelerated RTL Simulation"
date: 2026-02-15
summary: "RTL simulation has been CPU-only for decades. skalp puts it on the GPU — starting with Metal on Apple Silicon, where unified memory means zero DMA. How SharedCodegen produces both Metal shaders and compiled C++ from the same core, what the simulation step looks like on a GPU, and why fault simulation at 10M faults/sec is embarrassingly parallel."
tags: ["skalp", "simulation", "gpu", "metal", "performance"]
ShowToc: true
---

Every RTL simulator you've used — VCS, Xcelium, ModelSim, Verilator — runs on a CPU. This has been true for over thirty years. The simulation engine might be interpreted or compiled, single-threaded or partitioned across cores, but the actual computation always happens on general-purpose processors.

There's a reason for this: HDL simulation involves complex scheduling, event processing, and testbench interaction that are inherently sequential. But if you look at where the cycles actually go, most of the work is evaluating combinational logic. Given a set of input values and current register states, compute all the output signals. No side effects, no internal state, deterministic. A pure function.

And here's the thing about pure functions with no data dependencies between instances: that's exactly the workload GPUs were built for. A combinational cone is no different from a pixel shader — take some inputs, compute an output, repeat a thousand times in parallel.

skalp puts RTL simulation on the GPU using Metal compute shaders. Not as a research prototype or a proof-of-concept paper, but as a production simulation backend that runs alongside a compiled CPU backend, producing bit-exact results from the same code generation core.

The key enabler is Apple Silicon's Unified Memory Architecture (UMA): CPU and GPU share the same physical memory, so there's zero DMA overhead when the testbench needs to read simulation results or write new stimulus values. The testbench calls `set_input("clk", &[1])`, the runtime writes directly to the GPU buffer, dispatches the compute kernel, waits for completion, and reads the results back through another pointer dereference. No staging buffers, no memory copies, no PCIe transfers. For discrete GPUs (CUDA), you'd need explicit DMA transfers across PCIe, which changes the performance calculus entirely — but the same code generation core makes the CUDA port a wrapper exercise when the time comes.

This post covers:

- Why CPU simulation hits a wall and what makes combinational logic a natural GPU workload
- The `SharedCodegen` architecture that produces both Metal shaders and compiled C++ from the same SIR
- How HDL bit widths map to Metal's fixed-width types (and the alignment bugs that result)
- The three Metal compute kernels: combinational, sequential, and batched
- Why Metal on Apple Silicon is the right starting point (UMA changes everything)
- The full simulation step — all six phases from register snapshot to clock update
- GPU fault simulation: the killer app, running 10M faults/sec on an M1 Max
- The compiled CPU backend for comparison and fallback
- What a CUDA port would look like (and why SharedCodegen makes it a wrapper exercise)
- Cone extraction for future multi-kernel parallelism

The code references in this post point to actual skalp source files. The Metal backend is in `skalp-sir/src/codegen/metal.rs`, the shared codegen in `skalp-sir/src/codegen/shared.rs`, the type system in `skalp-sir/src/codegen/types.rs`, the GPU runtime in `skalp-sim/src/gpu_runtime.rs`, the fault simulator in `skalp-sim/src/gpu_fault_simulator.rs`, the CPU runtime in `skalp-sim/src/compiled_cpu_runtime.rs`, and the cone extractor in `skalp-sim/src/cone.rs`.

---

## Why Simulation is Slow

Traditional RTL simulators have three fundamental bottlenecks that limit performance regardless of how much CPU hardware you throw at them.

**Event-driven scheduling overhead.** Simulators like VCS and Xcelium use event-driven evaluation — when a signal changes, they schedule evaluation of every block that depends on it. This scheduling itself is expensive: maintaining priority queues, propagating sensitivity lists, checking delta cycles. For a design with 10K signals, the runtime spends more time deciding what to evaluate next than actually evaluating it. The data structures required for event scheduling (time wheels, sensitivity lists, fan-out tables) are pointer-heavy and cache-hostile.

Consider what happens when a single input changes: the simulator walks the sensitivity list, marks affected blocks as dirty, inserts evaluation events into the time wheel, pops events in priority order, evaluates each block, checks if any outputs changed, and if so, repeats the process for downstream blocks. Each of these steps involves pointer chasing through heap-allocated data structures. For a single clock edge in a 50K-signal design, this can mean millions of pointer dereferences — the worst possible workload for a modern CPU where cache misses cost 100+ cycles.

**Interpretation overhead.** Most commercial simulators interpret an internal representation of the design. Each combinational node is represented as a data structure with an opcode and operand pointers. The simulator walks the node graph, dispatches each opcode, and executes the operation. This interpretation loop has high per-node overhead: branch prediction misses on the opcode dispatch, indirect loads for operand values, and no opportunity for the CPU to pipeline or vectorize across nodes.

Verilator breaks this pattern by compiling SystemVerilog to C++, then compiling the C++ to native code with `gcc` or `clang` at `-O2`. This is why Verilator is 10-100x faster than interpreted simulators — native code eliminates the dispatch overhead entirely, and the C++ compiler can vectorize, inline, and optimize across node boundaries. VCS and Xcelium also have compiled modes that provide similar speedups. But even compiled simulators evaluate one combinational cone at a time, in topological order, on a single thread. The compilation helps with per-node overhead but doesn't address the fundamental sequential execution model.

skalp's compiled CPU backend follows the same approach as Verilator — generate C++, compile with `clang++ -O2`, load as a dynamic library — but adds the GPU backend on top, sharing the same code generation core.

**Cache-unfriendly memory access patterns.** A typical design has signals scattered across multiple data structures. Evaluating one combinational block reads inputs from one region of memory, writes outputs to another, and touches intermediate signals in yet another. This scattered access pattern thrashes CPU caches. L1 hit rates below 60% are common for large designs. As designs grow past ~100K signals, the working set exceeds L2/L3 capacity and simulation speed degrades superlinearly — a design that's 2x larger runs 3-4x slower, not 2x.

The key insight behind GPU simulation is that combinational cones are pure functions. They have no internal state — they read inputs and register values, compute outputs, and that's it. There are no loop-carried dependencies between cones, no shared mutable state, no synchronization needed. This is the textbook definition of an embarrassingly parallel workload. And GPU architectures are designed precisely to hide memory latency through massive parallelism — thousands of threads in flight means the hardware can switch to another thread while one waits on memory, keeping the ALUs busy.

**The industry has explored GPU simulation before.** Academic papers from the 2010s demonstrated 10-50x speedups for gate-level fault simulation on CUDA. Cadence and Synopsys have research groups exploring GPU acceleration. But no commercial tool has shipped a GPU simulation backend for RTL-level design simulation. The reasons are partly practical (the installed base is Linux servers with Xeons, not GPU workstations), partly architectural (existing simulator architectures can't easily extract the parallelism — the event-driven model is deeply baked in), and partly economic (customers want verification features more than raw speed). skalp starts from scratch, so it can design the representation and code generation around GPU execution from day one.

---

## The SharedCodegen Architecture

The architectural keystone of skalp's simulation backends is `SharedCodegen` — a single code generation core that produces both Metal compute shaders and compiled C++ from the same SIR (skalp Intermediate Representation). The SIR is the lowered form of a skalp design: combinational nodes in topological order, sequential nodes with explicit flip-flop semantics, typed signal declarations, and state element definitions. It's the point where "hardware description" becomes "computation to be executed."

The key design decision: instead of having a Metal backend and a C++ backend that independently generate code from the SIR, both backends share a single expression generator. The Metal backend wraps it with `kernel void` and `device` qualifiers; the C++ backend wraps it with `extern "C"` and standard types. But the computational core — the body of the function that actually evaluates combinational logic — is generated once by `SharedCodegen` and used by both.

Here's the pipeline:

```
                        ┌──────────────────┐
                        │    SIR Module     │
                        │  (combinational + │
                        │   sequential)     │
                        └────────┬─────────┘
                                 │
                        ┌────────▼─────────┐
                        │  SharedCodegen    │
                        │  (expressions,   │
                        │   struct layouts, │
                        │   eval bodies)    │
                        └──┬───────────┬───┘
                           │           │
                  ┌────────▼──┐   ┌───▼────────┐
                  │ MetalBack │   │  CppBack   │
                  │   end     │   │   end      │
                  └────┬──────┘   └────┬───────┘
                       │               │
              ┌────────▼──┐     ┌─────▼───────┐
              │  .metal   │     │    .cpp      │
              │  shader   │     │   source     │
              └────┬──────┘     └─────┬───────┘
                   │                  │
              ┌────▼──────┐     ┌─────▼───────┐
              │  Metal    │     │  clang++    │
              │  compiler │     │    -O2      │
              └────┬──────┘     └─────┬───────┘
                   │                  │
              ┌────▼──────┐     ┌─────▼───────┐
              │  GPU      │     │   .dylib    │
              │  pipeline │     │  (dlopen)   │
              └───────────┘     └─────────────┘
```

The `SharedCodegen` struct holds a reference to the SIR module and a `TypeMapper` configured for the target backend:

```
pub struct SharedCodegen<'a> {
    pub module: &'a SirModule,
    pub type_mapper: TypeMapper,
    output: String,
    indent: usize,
    in_batched_mode: bool,
    signal_width_cache: HashMap<String, usize>,
}
```

The Metal backend is a thin wrapper that adds kernel signatures and address space qualifiers:

```
pub struct MetalBackend<'a> {
    shared: SharedCodegen<'a>,
}

impl<'a> MetalBackend<'a> {
    pub fn new(module: &'a SirModule) -> Self {
        Self {
            shared: SharedCodegen::new(module, BackendTarget::Metal),
        }
    }

    pub fn generate(module: &SirModule) -> String {
        let mut backend = MetalBackend::new(module);
        backend.generate_shader()
    }
}
```

The C++ backend does the same — wraps `SharedCodegen` with `BackendTarget::Cpp`, adds `extern "C"` linkage and `uint32_t`/`uint64_t` type aliases instead of Metal's `uint`/`uint2`/`uint4`.

The `BackendTarget` enum controls the divergence:

```
pub enum BackendTarget {
    Metal,
    Cpp,
}
```

When `SharedCodegen` generates struct definitions, expressions, and evaluation bodies, the code is identical across backends. The only differences are: Metal gets `device`/`constant` address space qualifiers and `kernel void` entry points; C++ gets `extern "C"` and standard type names. The expressions inside kernel bodies — every add, shift, mux, comparison, bit-extract — are character-for-character the same.

The `SharedCodegen` core handles a substantial amount of complexity: binary and unary operations, type conversions, array indexing, widening multiplies, signed arithmetic, floating-point operations (stored as integer bits for bit-level semantics), concatenation, bit extraction, and conditional expressions. All of these are generated once, backend-agnostically.

The signal width cache is particularly important. It's built at construction time by walking all sources of width information in the SIR module:

1. Declared signals (from the module's signal list)
2. Input ports (from the module's input list)
3. Output ports (from the module's output list)
4. State elements (from the module's state element map, using `sir_type.width()` for arrays)
5. Combinational node outputs (computed from the node's operation in topological order)
6. Sequential node outputs (preserving declared width for flip-flop outputs)

This cache ensures that every expression uses the correct width without guessing or defaulting to 32. Getting a width wrong doesn't cause a compilation error — it causes a silent correctness bug where a 16-bit value gets truncated to 8 bits, or a 48-bit value gets zero-extended to 64 bits in the wrong place.

The Metal backend's `generate_shader()` method shows how thin the wrapper really is:

```
fn generate_shader(&mut self) -> String {
    let mut output = String::new();

    // Metal header
    output.push_str("#include <metal_stdlib>\n");
    output.push_str("#include <metal_compute>\n");
    output.push_str("using namespace metal;\n\n");

    // Generate struct definitions using shared codegen
    self.shared.generate_inputs_struct();
    self.shared.generate_registers_struct();
    self.shared.generate_signals_struct();
    output.push_str(&self.shared.take_output());

    // Generate three kernels
    output.push_str(&self.generate_combinational_kernel());
    output.push_str(&self.generate_sequential_kernel());
    output.push_str(&self.generate_batched_kernel());

    output
}
```

The struct definitions, the combinational body, the sequential body — all generated by `shared`. The Metal backend just provides the kernel wrappers and Metal-specific header.

This isn't just convenient. It's a correctness guarantee. If the Metal backend produces different results from the CPU backend for any input, there's a bug. And because both backends share the same expression generator, that class of bug is structurally impossible (alignment and type mapping bugs notwithstanding — more on those shortly).

---

## Type Mapping: Bits to Metal

HDL signals have arbitrary bit widths. A counter might be 4 bits, an address bus 32 bits, a hash 256 bits. Metal compute shaders need fixed-width types. The `TypeMapper` handles this conversion.

For Metal:

| Signal Width | Metal Type | Size | Alignment |
|---|---|---|---|
| 1–32 bits | `uint` | 4 bytes | 4 bytes |
| 33–64 bits | `uint2` | 8 bytes | 8 bytes |
| 65–128 bits | `uint4` | 16 bytes | 16 bytes |
| 129+ bits | `uint[N]` | N×4 bytes | 4 bytes |

For C++:

| Signal Width | C++ Type | Size |
|---|---|---|
| 1–32 bits | `uint32_t` | 4 bytes |
| 33–64 bits | `uint64_t` | 8 bytes |
| 65+ bits | `uint32_t[N]` | N×4 bytes |

The mapping code is straightforward:

```
fn get_metal_type_for_width(&self, width: usize) -> (String, Option<usize>) {
    match width {
        0 => ("uint".to_string(), None),
        1..=32 => ("uint".to_string(), None),
        33..=64 => ("uint2".to_string(), None),
        65..=128 => ("uint4".to_string(), None),
        _ => {
            let array_size = width.div_ceil(32);
            ("uint".to_string(), Some(array_size))
        }
    }
}
```

Both backends use the same `TypeMapper` — the only difference is the type name strings (`uint` vs `uint32_t`, `uint2` vs `uint64_t`). This means bit-level agreement between GPU and CPU is guaranteed by construction for scalar types.

There's one asymmetry worth noting: Metal has `uint4` (a built-in 128-bit SIMD type), but C++ doesn't have a standard 128-bit integer. So the C++ backend maps 65-128 bit signals to `uint32_t[N]` arrays, while Metal uses the native `uint4`. The generated expressions handle this differently — Metal can do `uint4 result = a + b` directly, while C++ must implement multi-word arithmetic element by element. `SharedCodegen` handles this by checking the backend target when generating wide arithmetic operations, but the bit-level results are identical.

For signals wider than 128 bits (a 256-bit hash, for instance), both backends use arrays of `uint`/`uint32_t`. The array size is `width.div_ceil(32)` — a 256-bit signal becomes `uint[8]` on Metal, `uint32_t[8]` on C++. Identical layout, identical access patterns.

**Alignment is where the bugs live.** Metal requires natural alignment: `uint2` must be 8-byte aligned, `uint4` must be 16-byte aligned. When the GPU runtime allocates buffers and computes field offsets, it must match the padding that the Metal compiler inserts into structs. BUG #182 was exactly this — the Rust side computed struct sizes without accounting for alignment padding, so register values were read from wrong offsets. The fix was explicit alignment calculation in every buffer access:

```
fn get_metal_type_alignment(&self, width: usize) -> usize {
    match width {
        1..=32 => 4,    // uint aligns to 4 bytes
        33..=64 => 8,   // uint2 aligns to 8 bytes
        65..=128 => 16, // uint4 aligns to 16 bytes
        _ => 4,         // Arrays of uint align to 4 bytes
    }
}
```

Every buffer read and write in `gpu_runtime.rs` now computes the aligned offset explicitly, matching what the Metal compiler does on the GPU side. The pattern looks like this in every buffer access function:

```
let metal_size = self.get_metal_type_size(input.width);
let metal_align = self.get_metal_type_alignment(input.width);

// Align offset to the required alignment boundary
let remainder = offset % metal_align;
if remainder != 0 {
    offset += metal_align - remainder;
}
offset += metal_size;
```

This is the kind of bug that only manifests with specific signal width combinations — a design with all 32-bit signals works fine (4-byte alignment everywhere, no padding needed), but add one 48-bit signal (`uint2`, 8-byte alignment) and every field after it reads from the wrong offset. The symptom is that register values look like garbage or are shifted — a flip-flop that should hold `0x42` reads `0x00` because the Rust side reads from byte offset 4 while the Metal side wrote to byte offset 8.

The fix was tedious but straightforward: add alignment calculation to `calculate_input_size()`, `calculate_register_size()`, `calculate_signal_size()`, `set_input()`, `get_output()`, and `capture_outputs()`. The same calculation, repeated in six places. An argument for generating the offset table once and sharing it, but for now explicit calculation at each site is more debuggable.

**Float types add another wrinkle.** HDL float signals (half, float, double) are stored as their integer bit representation (`uint16_t`, `uint32_t`, `uint64_t` in C++) to preserve bit-level semantics during copy operations between input/register/signal buffers. Float arithmetic uses explicit union-based bitcasts. This means a `float32` signal occupies 4 bytes with `uint` alignment, not `float` alignment — a distinction that matters on some architectures but fortunately not on Apple Silicon where both align to 4 bytes.

---

## Three Kernels

The Metal backend generates three compute kernels from the SIR. Each serves a different phase of the simulation step.

**`combinational_cone_0`** evaluates all combinational logic in topological order. It reads inputs and current register values, writes computed signals:

```
kernel void combinational_cone_0(
    device const Inputs* inputs [[buffer(0)]],
    device const Registers* registers [[buffer(1)]],
    device Signals* signals [[buffer(2)]],
    uint tid [[thread_position_in_grid]]
) {
    // ... generated combinational evaluation body
    // All expressions produced by SharedCodegen
}
```

Buffer bindings are fixed: `Inputs` at slot 0, `Registers` at slot 1, `Signals` at slot 2. The `Inputs`, `Registers`, and `Signals` structs are generated by `SharedCodegen` based on the SIR module's port, state element, and signal declarations. Fields are sorted alphabetically for deterministic layout — this ensures the Rust runtime and the Metal compiler agree on field order and offsets.

A generated `Registers` struct for a simple counter design might look like:

```
struct Registers {
    uint counter;
    uint state_reg;
};
```

For a FIFO design with 8-entry memory and pointer/gray-code tracking:

```
struct Registers {
    uint fifo_mem_0_value;
    uint fifo_mem_1_value;
    // ... fifo_mem_2-7_value ...
    uint fifo_rd_ptr;
    uint fifo_rd_ptr_gray;
    uint fifo_rd_ptr_gray_sync1;
    uint fifo_rd_ptr_gray_sync2;
    uint fifo_wr_ptr;
    uint fifo_wr_ptr_gray;
    uint fifo_wr_ptr_gray_sync1;
    uint fifo_wr_ptr_gray_sync2;
};
```

The alphabetical ordering means `fifo_mem_0_value` comes before `fifo_rd_ptr`, which comes before `fifo_wr_ptr`. Both the Metal compiler and the Rust runtime must agree on this order — any mismatch means the runtime reads the wrong field from the buffer.

**`sequential_update`** updates flip-flops on clock edges. It reads from a frozen snapshot of the registers (the "pre-edge" state) and writes new values to the working register buffer:

```
kernel void sequential_update(
    device const Inputs* inputs [[buffer(0)]],
    device const Registers* current_registers [[buffer(1)]],
    device const Signals* signals [[buffer(2)]],
    device Registers* next_registers [[buffer(3)]],
    uint tid [[thread_position_in_grid]]
) {
    // ... generated sequential update body
    // Reads current_registers, writes next_registers
}
```

The four-buffer design is critical for correctness. All flip-flops must see the pre-edge state simultaneously — if flip-flop A's output feeds flip-flop B's input, B must see A's old value, not its just-updated value. This matches real hardware behavior where all flip-flops sample on the same clock edge.

Consider a shift register: `Q0 → Q1 → Q2`. On a rising edge, Q2 should get Q1's old value, Q1 should get Q0's old value. If the kernel updated Q0 first (writing to the same buffer it reads), Q1 would see Q0's new value — and the shift register would copy one value to all stages instead of shifting. The double-buffer (read from `current_registers`, write to `next_registers`) eliminates this class of bug entirely. It's the same trick hardware designers use in RTL simulation testbenches — non-blocking assignments in SystemVerilog work the same way.

**`batched_simulation`** runs multiple cycles in a single GPU dispatch, amortizing kernel launch and synchronization overhead:

```
kernel void batched_simulation(
    device Inputs* inputs [[buffer(0)]],
    device Registers* registers [[buffer(1)]],
    device Signals* signals [[buffer(2)]],
    constant uint& num_cycles [[buffer(3)]],
    uint tid [[thread_position_in_grid]]
) {
    // Copy scalar registers to thread-local variables
    uint local_counter = registers->counter;
    uint local_state = registers->state;
    // ...

    for (uint cycle = 0; cycle < num_cycles; cycle++) {
        // Combinational evaluation (uses local_ vars)
        // Sequential update
    }

    // Write back local state
    registers->counter = local_counter;
    registers->state = local_state;
    // ...

    // Final combinational pass (FWFT semantics)
}
```

The batched kernel copies scalar registers (up to 128 bits) into thread-local variables, runs the simulation loop entirely in registers/local memory, and writes back at the end. This avoids repeated device memory round-trips within the loop. Array-type state elements (like memory banks in a FIFO) stay in device memory — they're too large for thread-local storage.

The local copy/writeback code is generated by `MetalBackend` based on the SIR module's state elements. For each scalar state element, it generates:

```
// Copy to local at kernel start
uint local_counter = registers->counter;
uint local_state_reg = registers->state_reg;
uint2 local_wide_value = registers->wide_value;

// ... simulation loop uses local_ variables ...

// Write back at kernel end
registers->counter = local_counter;
registers->state_reg = local_state_reg;
registers->wide_value = local_wide_value;
```

BUG #254 discovered that some flip-flop outputs aren't registered as state elements in the SIR (they're inferred from the sequential node graph). The fix: after iterating state elements, also scan sequential nodes for flip-flop outputs that aren't in the state element map, and generate local copies for those too.

The final combinational pass after the loop ensures output signals reflect the latest register state (FWFT — First Word Fall Through — semantics), so that outputs are never stale by one cycle. Without this pass, the outputs would reflect the register values from the second-to-last cycle of the batch.

---

## Why Metal First

There are two reasons skalp starts with Metal on Apple Silicon. The practical reason is that development happens on a MacBook Pro. The technical reason is UMA — Unified Memory Architecture — and it changes the economics of GPU simulation fundamentally.

On Apple Silicon, CPU and GPU share the same physical memory. There is no discrete VRAM, no PCIe bus, no separate memory controller for the GPU. The M1/M2/M3 memory subsystem serves both processors through a single unified controller. When skalp allocates a buffer with `StorageModeShared`, both the CPU and GPU access the same bytes at the same physical address:

```
self.input_buffer = Some(
    self.device
        .device
        .new_buffer(input_size.max(16), MTLResourceOptions::StorageModeShared),
);
```

After a GPU kernel writes results to a `StorageModeShared` buffer, the CPU reads them directly:

```
let ptr = register_buffer.contents() as *const u32;
let value = unsafe { *ptr.offset(0) };
```

No `memcpy`. No staging buffers. No DMA transfer. The pointer returned by `buffer.contents()` is a regular virtual address that both processors use. The cost of reading a GPU result from the CPU is a cache line fetch — the same cost as reading any other memory location.

This shows up everywhere in the GPU runtime. Setting an input value writes directly to the Metal buffer:

```
let input_ptr = input_buffer.contents() as *mut u8;
unsafe {
    std::ptr::write_bytes(input_ptr.add(offset), 0, metal_size);
    std::ptr::copy_nonoverlapping(
        value.as_ptr(),
        input_ptr.add(offset),
        bytes_needed.min(metal_size),
    );
}
```

Reading a register value after a GPU kernel completes:

```
let register_ptr = register_buffer.contents() as *const u8;
let mut value = vec![0u8; bytes_needed];
unsafe {
    std::ptr::copy_nonoverlapping(
        register_ptr.add(offset),
        value.as_mut_ptr(),
        bytes_needed.min(metal_size),
    );
}
```

These are plain memory operations. The `contents()` call returns a raw pointer to the shared allocation. There's no command buffer submission, no fence, no synchronization beyond what the GPU runtime already does with `wait_until_completed()`. After the GPU kernel finishes, the data is just there in memory.

**On a discrete GPU, the picture is completely different.** A CUDA-based simulation would have the GPU's VRAM on one side of a PCIe bus and the CPU's DRAM on the other. Every time the CPU needs to read a simulation result — to check an output value, to decide whether to inject a stimulus, to log a waveform — it needs a DMA transfer across PCIe.

The numbers: PCIe 4.0 x16 has ~25 GB/s bandwidth, but that's throughput for large, streaming transfers. For small transfers (a few kilobytes of register state), latency dominates. A `cudaMemcpy` for a 4KB buffer takes roughly 5-10μs including the kernel launch latency (~5μs) plus transfer time (~160 nanoseconds per 4KB at PCIe latency). The kernel launch alone costs more than the actual data transfer.

For skalp's per-step simulation — where the testbench runs on the CPU and decides the next input values based on current outputs — the DMA cost would dominate. At 10μs per step, the maximum simulation rate is 100K steps/second regardless of how fast the GPU evaluates the logic. A design that takes 100 nanoseconds to evaluate combinationally would still take 10μs per step because of the DMA bookends. On Apple Silicon with UMA, the same operation costs nothing beyond the kernel execution itself.

The batched simulation kernel mitigates the DMA cost for CUDA by running many cycles in a single dispatch (one DMA at the start, one at the end, all intermediate cycles stay on-GPU). But this only works when the testbench doesn't need per-cycle visibility — regression runs, not interactive debugging. For interactive debugging where the user wants to inspect state after each clock edge, discrete GPU simulation would need to fall back to the CPU backend.

**SharedCodegen makes the CUDA port a wrapper exercise, not a rewrite.** The Metal backend is 312 lines. The C++ backend is similar. All the computation logic — struct generation, expression evaluation, topological ordering — lives in the 3,200-line SharedCodegen. Adding a CUDA backend means writing a new 300-line wrapper that emits `__global__ void` instead of `kernel void`, uses `blockIdx.x * blockDim.x + threadIdx.x` instead of `uint tid [[thread_position_in_grid]]`, and handles memory differently. The expressions inside the kernel body don't change at all.

---

## The Simulation Step

Before we can step, the runtime must be initialized. The `initialize()` method generates the Metal shader from the SIR module, compiles three pipeline states (combinational, sequential, batched), allocates and zero-initializes all GPU buffers, and identifies clock signals from the module's `clock_domains` and sequential nodes. The shader source is also written to `/tmp/skalp_metal_shader.metal` for debugging — inspecting the generated Metal code is invaluable when tracking down simulation mismatches.

The `step()` method in `GpuRuntime` executes one simulation cycle. It has six phases, and the ordering matters for correctness:

```
                  step() begins
                       │
              ┌────────▼────────┐
         1.   │  Create register │  Copy main → shadow
              │    snapshot      │  (shadow = frozen pre-edge state)
              └────────┬────────┘
                       │
              ┌────────▼────────┐
         2.   │  Combinational   │  Inputs + old registers → signals
              │  (old state)     │  (GPU dispatch, all cones batched)
              └────────┬────────┘
                       │
              ┌────────▼────────┐
         3.   │  Sequential      │  Only on rising edge
              │  update          │  Reads shadow (frozen), writes main
              └────────┬────────┘
                       │
              ┌────────▼────────┐
         4.   │  Combinational   │  Inputs + new registers → signals
              │  (new state)     │  (FWFT: outputs reflect updates)
              └────────┬────────┘
                       │
              ┌────────▼────────┐
         5.   │  Capture         │  Copy signal outputs → output buffer
              │  outputs         │  (with bit-width masking)
              └────────┬────────┘
                       │
              ┌────────▼────────┐
         6.   │  Update clock    │  Shift current → previous
              │  previous values │  (prevents false edge detection)
              └────────┘
```

**Phase 1: `create_register_snapshot()`** copies the current register buffer to the shadow buffer. This frozen snapshot is what the sequential kernel reads from. Without this, the sequential kernel would read partially-updated register values — flip-flop A updates, then flip-flop B reads A's new value instead of its pre-edge value. The snapshot guarantees all flip-flops see consistent pre-edge state, matching real hardware semantics where all flip-flops sample on the same clock edge.

```
fn create_register_snapshot(&mut self) {
    if let (Some(main_buffer), Some(shadow_buffer)) =
        (&self.register_buffer, &self.shadow_register_buffer)
    {
        let size = main_buffer.length() as usize;
        let main_ptr = main_buffer.contents() as *const u8;
        let shadow_ptr = shadow_buffer.contents() as *mut u8;
        unsafe {
            std::ptr::copy_nonoverlapping(main_ptr, shadow_ptr, size);
        }
    }
}
```

On Apple Silicon with UMA, this is a CPU-side `memcpy` of the register buffer — no GPU involvement, no DMA. The source and destination are both in shared memory, so this is a plain memory copy at memory bandwidth (>100 GB/s on M1 Max). For a typical register buffer of a few hundred bytes, this takes nanoseconds.

On a discrete GPU, this would be a device-to-device copy (`cudaMemcpyDeviceToDevice`) — fast (~500 GB/s on an A100) but still requires a CUDA API call and synchronization.

**Phase 2: `execute_combinational()`** dispatches all combinational cone kernels in a single batched command buffer:

```
async fn execute_combinational(&mut self) -> Result<(), SimulationError> {
    let cone_count = self.cached_cone_count;
    if cone_count == 0 {
        return Ok(());
    }

    let command_buffer = self.device.command_queue.new_command_buffer();

    for i in 0..cone_count {
        let pipeline_name = format!("combinational_{}", i);
        if let Some(pipeline) = self.pipelines.get(&pipeline_name) {
            let encoder = command_buffer.new_compute_command_encoder();
            encoder.set_compute_pipeline_state(pipeline);

            if let Some(input_buffer) = &self.input_buffer {
                encoder.set_buffer(0, Some(input_buffer), 0);
            }
            if let Some(register_buffer) = &self.register_buffer {
                encoder.set_buffer(1, Some(register_buffer), 0);
            }
            if let Some(signal_buffer) = &self.signal_buffer {
                encoder.set_buffer(2, Some(signal_buffer), 0);
            }

            let thread_groups = metal::MTLSize { width: 1, height: 1, depth: 1 };
            let threads_per_group = metal::MTLSize { width: 64, height: 1, depth: 1 };
            encoder.dispatch_thread_groups(thread_groups, threads_per_group);
            encoder.end_encoding();
        }
    }

    command_buffer.commit();
    command_buffer.wait_until_completed();
    Ok(())
}
```

The critical performance detail: all cones are encoded into a single command buffer with a single `commit()` and `wait_until_completed()`. Each cone gets its own compute encoder within the same command buffer, but the GPU executes them all in a single submission.

Earlier versions submitted one command buffer per cone with a synchronous wait each time. For a simulation step that calls `execute_combinational()` twice (phases 2 and 4), this meant `2 × cone_count` GPU round-trips per step. With 50 cones (not uncommon in larger designs), that's 100 round-trips — each costing ~5-10μs of synchronization overhead. The fix was straightforward: batch all cones into one command buffer. Now it's 2 round-trips per step (one per combinational pass), regardless of cone count.

**Phase 3: `execute_sequential()`** only runs on a rising clock edge. It reads from the shadow buffer (frozen pre-edge snapshot) and writes to the working register buffer.

Edge detection iterates through all registered clocks (identified during initialization from the SIR module's `clock_domains` and sequential nodes), reading the current clock value from the input buffer at the correct offset:

```
let current_value = unsafe { *ptr.add(input_offset) } != 0;
let prev_value = self.clock_manager.clocks.get(&input.name)
    .map(|c| c.previous_value).unwrap_or(false);

if current_value && !prev_value {
    // Rising edge detected
    has_edge = true;
}
```

BUG #179 was a subtle offset bug here — the code originally always read from offset 0 in the input buffer, meaning only the first input (which happened to be the clock in simple designs) was correctly detected. Multi-clock designs (like an async FIFO with separate read and write clocks) would only see edges on one clock. The fix: track `input_offset` as we iterate through inputs, advancing by `(input.width + 31) / 32` u32 words for each input.

The sequential kernel dispatch uses the double-buffer binding described in the Three Kernels section:

```
// Buffer 1: Frozen snapshot (pre-edge state, read-only)
if let Some(shadow_buffer) = &self.shadow_register_buffer {
    encoder.set_buffer(1, Some(shadow_buffer), 0);
}

// Buffer 3: Working register buffer (receives updates, write-only)
if let Some(register_buffer) = &self.register_buffer {
    encoder.set_buffer(3, Some(register_buffer), 0);
}
```

This binding is the key to correctness. The sequential kernel reads `current_registers` (the frozen snapshot) and writes `next_registers` (the working buffer). Every flip-flop sees the same pre-edge state, regardless of evaluation order. In real hardware, all flip-flops sample on the same clock edge — they don't see each other's updates. The double-buffer makes the software simulation match this behavior.

**Phase 4: `execute_combinational()` again** — the second combinational pass recomputes all signals using the newly-updated register values. This provides FWFT semantics: if a flip-flop updates a register that drives an output, the output reflects the new value in the same step. Without this pass, outputs would be stale by one cycle.

This is particularly important for FIFO implementations. When data is written to a FIFO and the write pointer advances, the `full`/`empty` flags should reflect the new pointer value immediately — not wait until the next clock edge. The second combinational pass propagates the updated register values through the combinational logic that computes these flags.

**Phase 5: `capture_outputs()`** copies output signal values from the signal buffer to a dedicated output buffer, applying bit-width masking. Metal's `~` operator produces a full 32-bit NOT, but a 4-bit signal should mask to `0xF`:

```
if output.width < 32 {
    let raw_value = *(signal_ptr.add(signal_offset) as *const u32);
    let mask = (1u32 << output.width) - 1;
    let masked_value = raw_value & mask;
    *(output_ptr.add(output_offset) as *mut u32) = masked_value;
}
```

**Phase 6: `update_clock_previous_values()`** shifts the current clock value into the clock manager's `previous_value` field. This is done by reading the clock values from the input buffer and calling `set_clock()` with the current value, which internally shifts `current_value` to `previous_value`:

```
fn update_clock_previous_values(&mut self) {
    if let Some(module) = &self.module {
        if let Some(input_buffer) = &self.input_buffer {
            let ptr = input_buffer.contents() as *const u32;
            let mut input_offset = 0usize;

            for input in &module.inputs {
                let is_clock = self.clock_manager.clocks.contains_key(&input.name);
                if is_clock {
                    let current_value = unsafe { *ptr.add(input_offset) } != 0;
                    self.clock_manager.set_clock(&input.name, current_value);
                }
                input_offset += (input.width + 31) / 32;
            }
        }
    }
}
```

Without this (BUG #180), if the testbench didn't call `set_input()` between steps, the clock manager would still see `previous_value = 0` and incorrectly detect a rising edge every step. The symptom was that a state machine would advance through all its states in a single simulation run, because every step looked like a clock edge. A testbench that set the clock high once and then stepped 10 times would see 10 state transitions instead of 1.

The overall step flow can be summarized as: **snapshot → compute(old) → update(edge) → compute(new) → capture → clock**. This six-phase protocol is the same on both the GPU and CPU backends. The GPU backend uses Metal command buffers and compute dispatches; the CPU backend uses function pointer calls. But the logical flow is identical, and the results are bit-exact.

For multi-cycle execution without per-step visibility, the `run_batched()` method bypasses the step-by-step protocol entirely. It dispatches the batched simulation kernel with a cycle count parameter, and the GPU executes all cycles internally with its own combinational/sequential loop and local register copies. This avoids the per-cycle overhead of six phases and produces only the final state:

```
pub async fn run_batched(&mut self, cycles: u64) -> SimulationResult<SimulationState> {
    if let Some(pipeline) = self.pipelines.get("batched") {
        // Set cycle count in params buffer
        if let Some(params_buffer) = &self.params_buffer {
            let params_ptr = params_buffer.contents() as *mut u32;
            unsafe { *params_ptr = cycles as u32; }
        }

        // Single GPU dispatch for all cycles
        let command_buffer = self.device.command_queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(pipeline);
        // ... set buffers ...
        encoder.dispatch_thread_groups(thread_groups, threads_per_group);
        encoder.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        self.current_cycle += cycles;
        self.capture_outputs()?;
    }
    Ok(self.extract_state())
}
```

One GPU dispatch, one wait, regardless of whether you run 100 cycles or 100,000. This is where GPU simulation truly shines — the overhead is constant, and all the computation happens in the GPU's tight loop with local register variables.

---

## Fault Simulation: Embarrassingly Parallel

GPU fault simulation is the killer app — the workload where GPU acceleration delivers orders of magnitude improvement over CPU.

The concept is simple: take a design, inject one fault (stuck-at-0, stuck-at-1, bit-flip, or transient), simulate for N cycles, check if any detection signal fires. Repeat for every fault in the campaign.

The crucial property: each fault simulation is completely independent. Thread 0 injects a stuck-at-0 on primitive 0 and simulates 100 cycles. Thread 1 injects a stuck-at-1 on primitive 0 and simulates 100 cycles. Thread 2 does stuck-at-0 on primitive 1. And so on. No inter-thread communication, no shared mutable state, no barrier synchronization. Each thread has its own copy of the signal state array, its own fault configuration, its own result slot. One GPU thread per fault.

```
┌─────────────────────────────────────────────────────────────┐
│                    Fault Campaign                           │
│  [Fault0] [Fault1] [Fault2] ... [FaultN]                   │
└─────────────────────┬───────────────────────────────────────┘
                      │ GPU dispatch (one thread per fault)
                      ▼
┌─────────────────────────────────────────────────────────────┐
│                   Metal Compute Shader                      │
│  thread[0]: simulate(design, fault0, vectors) → detected0  │
│  thread[1]: simulate(design, fault1, vectors) → detected1  │
│  thread[2]: simulate(design, fault2, vectors) → detected2  │
│  ...                                                        │
└─────────────────────────────────────────────────────────────┘
```

The data structures are packed for GPU efficiency. Each primitive and fault is a fixed-size C-compatible struct:

```
#[repr(C)]
struct GpuPrimitive {
    ptype: u32,
    inputs: [u32; 4],
    num_inputs: u32,
    output: u32,
    _pad: [u32; 2],
}

#[repr(C)]
struct GpuFault {
    target_primitive: u32,
    fault_type: u32,   // 0=SA0, 1=SA1, 2=BitFlip, 3=Transient
    inject_cycle: u32,
    duration: u32,      // 0 = permanent
}
```

The fault simulation kernel gives each thread its own local signal state array. The thread evaluates all primitives in order for each cycle, applying the fault when the target primitive is reached:

```
kernel void fault_sim_kernel(
    device const Primitive* primitives [[buffer(0)]],
    device const Fault* faults [[buffer(1)]],
    device FaultResult* results [[buffer(2)]],
    device const uint* golden_outputs [[buffer(3)]],
    constant uint& num_primitives [[buffer(4)]],
    constant uint& num_cycles [[buffer(5)]],
    constant uint& num_outputs [[buffer(6)]],
    device const uint* detection_signal_ids [[buffer(7)]],
    constant uint& num_detection_signals [[buffer(8)]],
    uint tid [[thread_position_in_grid]]
) {
    Fault fault = faults[tid];
    uint signals[NUM_SIGNALS];

    // Initialize and simulate
    for (uint cycle = 0; cycle < num_cycles; cycle++) {
        for (uint p = 0; p < num_primitives; p++) {
            uint result = eval_gate(primitives[p], signals);
            result = apply_fault(result, fault, p, cycle);
            signals[primitives[p].output] = result;
        }
        // Check detection signals
    }

    results[tid].detected = detected;
    results[tid].detection_cycle = detection_cycle;
}
```

Each thread's signal state array is allocated in GPU thread-local memory (Metal's `thread` address space, implicitly). For a design with 200 signals, that's 800 bytes per thread. An M1 Max with 32 GPU cores running 64 threads per threadgroup can have thousands of threads in flight simultaneously, each with its own 800-byte signal array. The GPU's register file and L1 cache handle this efficiently — it's exactly the kind of workload GPUs were designed for.

Buffer allocation for the fault campaign uses the same `StorageModeShared` pattern as the simulation runtime:

```
let fault_buffer = self.device.new_buffer(
    fault_buffer_size.max(16) as u64,
    MTLResourceOptions::StorageModeShared,
);
unsafe {
    let ptr = fault_buffer.contents() as *mut GpuFault;
    std::ptr::copy_nonoverlapping(gpu_faults.as_ptr(), ptr, gpu_faults.len());
}
```

Populate on the CPU side, dispatch to GPU, read results back — all through the same shared memory. No staging, no DMA.

**Performance numbers.** On an M1 Max (32 GPU cores, 400 GB/s memory bandwidth): ~10M fault simulations per second. On an M2 Ultra (76 GPU cores): ~20M fault simulations per second. A typical design with 10K primitives testing stuck-at-0 and stuck-at-1 for every primitive generates 20K faults — completed in about 2 seconds on an M1 Max.

**The industry comparison is stark.** Synopsys Z01X and Cadence Modus are the leading commercial fault simulators. They use multi-threaded CPU execution — typically 8 to 64 cores of Xeon or EPYC, with sophisticated partitioning and workload balancing across fault groups. A 64-core Xeon server running Z01X might process ~500K fault simulations per second, which is impressive engineering but fundamentally limited by the number of CPU cores.

A GPU has thousands of execution units. An M1 Max has 32 GPU cores, each with multiple execution units capable of running 64+ threads simultaneously. An NVIDIA A100 has 6,912 CUDA cores. The GPU approach doesn't just parallelize better — it eliminates the partitioning and synchronization overhead entirely because there's nothing to partition. Each fault is self-contained: its own signal state array, its own evaluation loop, its own result. No shared state, no locks, no critical sections, no workload rebalancing. The scheduler just launches N threads and waits.

The GPU kernel includes a complete gate evaluator (`eval_gate`) that handles all primitive types — AND, OR, XOR, NAND, NOR, XNOR, INV, BUF, MUX2 — with variable input counts. It also has a fault injector (`apply_fault`) that checks whether the current primitive and cycle match the fault configuration and applies the appropriate modification:

```
uint apply_fault(uint value, Fault fault, uint prim_idx, uint cycle) {
    if (prim_idx != fault.target_primitive) return value;
    if (cycle < fault.inject_cycle) return value;
    if (fault.duration > 0 && cycle >= fault.inject_cycle + fault.duration)
        return value;

    switch (fault.fault_type) {
        case FAULT_SA0: return 0;
        case FAULT_SA1: return 1;
        case FAULT_BITFLIP: return ~value & 1;
        case FAULT_TRANSIENT:
            if (cycle == fault.inject_cycle) return ~value & 1;
            return value;
    }
}
```

The Rust-side `GpuFaultSimulator` handles all the bookkeeping: generating the complete fault list (every primitive × every fault type), packing into `GpuFault` structs, allocating and populating the GPU buffers (`StorageModeShared`, of course), dispatching the kernel with the right threadgroup size, and reading back the `GpuFaultResult` array to compute diagnostic coverage metrics.

The fault simulator also supports multiple detection modes. A signal annotated with `#[detection_signal]` in the skalp source can operate in continuous mode (any assertion triggers detection) or windowed mode (check only at specific cycles). The kernel tracks which detection signal fired and at which cycle, feeding back into ISO 26262 diagnostic coverage metrics. The results are broken down by fault category (permanent vs. transient) with per-category FIT rate calculations, safe fault percentages, and residual FIT contributions — everything needed for an ISO 26262 FMEDA (Failure Modes Effects and Diagnostic Analysis).

---

## The Compiled CPU Backend

For comparison, validation, and fallback, skalp has a compiled CPU backend that follows the same pipeline: SIR module → C++ code generation → `clang++ -O2` → `.dylib` → `dlopen` → function pointer calls.

The C++ backend uses the same `SharedCodegen` core with `BackendTarget::Cpp`. It generates a source file with three functions and an export table:

```
type CombinationalEvalFn =
    unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void);
type SequentialUpdateFn =
    unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, *mut c_void);
type BatchedSimulationFn =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32);

#[repr(C)]
struct SkalpKernel {
    combinational_eval: CombinationalEvalFn,
    sequential_update: SequentialUpdateFn,
    batched_simulation: BatchedSimulationFn,
    inputs_size: usize,
    registers_size: usize,
    signals_size: usize,
}
```

The runtime loads the library, looks up the `SKALP_KERNEL` symbol, and calls the function pointers directly:

```
let kernel: *const SkalpKernel = unsafe {
    let sym: Symbol<*const SkalpKernel> = library
        .get(b"SKALP_KERNEL")
        .map_err(|e| CompileError::CompilationFailed(
            format!("Failed to find SKALP_KERNEL: {}", e)
        ))?;
    *sym
};
```

Buffer management uses aligned Rust `Vec<u8>` instead of Metal buffers, but the struct layouts are identical — same field ordering (alphabetical sort of state elements), same type sizes, same byte widths. The `AlignedBuffer` type aligns to 16 bytes for SIMD compatibility:

```
struct AlignedBuffer {
    data: Vec<u8>,
}

impl AlignedBuffer {
    fn new(size: usize) -> Self {
        let aligned_size = (size + 15) & !15;
        Self {
            data: vec![0u8; aligned_size.max(16)],
        }
    }
}
```

The CPU backend's `step()` follows the same three-phase pattern: combinational evaluation, sequential update on rising edge, combinational again for FWFT. Calling the compiled functions is just function pointer invocation through the export table:

```
fn eval_combinational(&mut self) {
    unsafe {
        let kernel = &*self.kernel;
        (kernel.combinational_eval)(
            self.inputs.as_ptr(),
            self.registers.as_ptr(),
            self.signals.as_mut_ptr(),
        );
    }
}
```

The sequential update uses the same double-buffered snapshot approach: copy current registers to shadow, then call the update function which reads from shadow and writes to the working buffer.

For batched execution, the CPU backend uses the compiled `batched_simulation` function directly when running more than 10 cycles — a single native function call that runs the entire loop in optimized C++:

```
if cycles > 10 {
    unsafe {
        let kernel = &*self.kernel;
        (kernel.batched_simulation)(
            self.inputs.as_mut_ptr(),
            self.registers.as_mut_ptr(),
            self.signals.as_mut_ptr(),
            cycles as u32,
        );
    }
}
```

This dual-backend architecture provides something no commercial tool has: bit-exact validation of GPU results against compiled native code from the same source representation. Run a testbench on both backends, compare every output at every cycle, and any divergence points directly to a runtime bug (alignment, buffer management, clock edge detection) rather than a computation bug. Because the computation logic is generated from the same `SharedCodegen` core, the expressions are identical — there's no way for `a + b` to produce different results between backends.

Adding a new backend — CUDA, Vulkan, WebGPU — means writing only the wrapper code. The 3,200 lines of expression generation, struct layout, and topological ordering come for free.

---

## What CUDA Would Look Like

`SharedCodegen` makes the CUDA port a wrapper exercise. The changes are mechanical, and they fall into two categories: syntax differences in the kernel wrapper, and memory model differences in the runtime.

**What changes in the kernel signatures:**

The combinational kernel in Metal:

```
kernel void combinational_cone_0(
    device const Inputs* inputs [[buffer(0)]],
    device const Registers* registers [[buffer(1)]],
    device Signals* signals [[buffer(2)]],
    uint tid [[thread_position_in_grid]]
) {
```

The same kernel in CUDA:

```
__global__ void combinational_cone_0(
    const Inputs* inputs,
    const Registers* registers,
    Signals* signals
) {
    uint tid = blockIdx.x * blockDim.x + threadIdx.x;
```

Metal's `device`/`constant` address space qualifiers disappear (CUDA uses implicit global memory). Metal's `[[buffer(N)]]` attribute bindings become explicit pointer parameters passed at launch time. Metal's `uint tid [[thread_position_in_grid]]` becomes the standard CUDA thread ID computation. The struct definitions (`Inputs`, `Registers`, `Signals`) are identical — same field names, same types (both use `uint`/`uint32_t`), same ordering.

**What stays the same:** every expression, every struct definition, every evaluation order. The combinational body that `SharedCodegen` generates is valid C++ in both contexts. A Metal add is `a + b`. A CUDA add is `a + b`. A Metal ternary mux is `sel ? a : b`. A CUDA ternary mux is `sel ? a : b`. Bit extractions, shifts, comparisons, array indexing — all syntactically identical. The generated code between the kernel opening brace and the closing brace is character-for-character the same.

**The memory model is where CUDA gets interesting.** There are three options, each with different performance characteristics:

1. **Explicit DMA** (`cudaMalloc` + `cudaMemcpy`). Fastest for batched simulation — copy inputs to GPU VRAM, run N cycles, copy results back. One DMA transfer per batch, all intermediate state stays in VRAM. This is the likely production path for regression runs. The per-step overhead of two `cudaMemcpy` calls (~10μs total round-trip) means interactive debugging should fall back to the compiled CPU backend.

2. **Managed memory** (`cudaMallocManaged`). CUDA handles page migration automatically between CPU and GPU memory. Simpler API — code looks almost like UMA. But higher overhead: the runtime migrates 4KB pages on access, which is much coarser than skalp's register buffers (typically a few hundred bytes). A single CPU read of a register value would migrate an entire 4KB page. For per-step simulation with frequent CPU reads, the page fault and migration overhead would exceed explicit DMA.

3. **Pinned + mapped** (`cudaHostAllocMapped`). Allocate in CPU-accessible pinned memory that the GPU can access directly over PCIe. Zero-copy from the CPU side (just dereference the pointer, like UMA), but GPU reads go over PCIe at high latency. Each GPU load of a signal value takes ~500ns instead of ~5ns for VRAM. Viable for small, infrequently-accessed buffers (the `num_cycles` constant, for instance) but not for the signal and register arrays that the kernel reads on every cycle.

For skalp, the path is option 1 with the batched kernel for regression runs and performance benchmarking. A CUDA `run_batched(10000)` would look like: `cudaMemcpyHostToDevice` for inputs and registers (one transfer, ~10μs), launch the batched kernel (10K cycles in one dispatch), `cudaMemcpyDeviceToHost` for registers and signals (~10μs). Total overhead: ~20μs for 10K cycles, or 2 nanoseconds per cycle — negligible. For interactive step-by-step debugging, the compiled CPU backend is the right choice regardless of GPU platform.

The `SharedCodegen` architecture means the CUDA backend wrapper would be roughly 300 lines — comparable to the 312-line Metal backend. A `CudaBackend` struct wrapping `SharedCodegen` with `BackendTarget::Cuda` (to be added), emitting `__global__` instead of `kernel`, and generating the launch configuration boilerplate.

---

## Cone Extraction

Before shader generation, skalp analyzes the design's combinational logic to identify parallelism opportunities. The `ConeExtractor` builds a dependency graph, finds strongly connected components, and groups combinational blocks into cones.

A combinational cone is a set of logic blocks that can execute together as a unit. Each cone has defined inputs (signals from other cones, registers, or primary inputs) and outputs (signals consumed by other cones, registers, or primary outputs):

```
pub struct CombinationalCone {
    pub id: ConeId,
    pub blocks: Vec<CombBlockId>,
    pub inputs: Vec<SirSignalId>,
    pub outputs: Vec<SirSignalId>,
    pub workgroup_size: u32,
    pub logic_depth: u32,
}
```

The extraction process has five steps:

1. **Build dependency graph.** For each combinational block, identify which other blocks drive its inputs. This creates a directed graph where edges represent data flow — block A feeds block B means A must execute before B.

2. **Find strongly connected components.** Using Kosaraju's algorithm (from the `petgraph` crate), identify cycles in the dependency graph. Blocks in a cycle must be in the same cone because they have circular dependencies. In practice, most SIR modules have no combinational cycles (the skalp compiler breaks feedback loops at register boundaries), so most SCCs are single blocks.

3. **Group into cones.** Each SCC becomes one cone. Single blocks become single-block cones. This is the initial partitioning.

4. **Optimize boundaries.** Merge small cones (below 16 blocks) with their neighbors to reduce kernel launch overhead. A cone with 3 blocks would waste GPU resources — the kernel launch cost exceeds the computation time. The optimizer also plans for splitting oversized cones (above 256 blocks) for better load balancing, though this isn't yet implemented.

5. **Determine execution order.** Build a dependency graph between cones (does cone B depend on any output of cone A?) and topological sort to get the execution order. Independent cones can execute in parallel.

The result includes a parallelism factor — the ratio of total blocks to maximum cone depth — which estimates how much speedup multi-cone dispatch could provide:

```
pub struct ConeExtractionResult {
    pub cones: Vec<CombinationalCone>,
    pub execution_order: Vec<ConeId>,
    pub parallelism_factor: f32,
}
```

A parallelism factor of 1.0 means all blocks are in a single chain (no parallelism possible). A factor of 10.0 means on average 10 blocks can execute in parallel. For a design with independent subsystems (e.g., a UART controller and a SPI controller on the same chip), the factor can be quite high — the two subsystems share no combinational dependencies and could execute on separate GPU cores.

The `ConeExtractor` also calculates per-cone metrics: workgroup size (from block-level hints or a default of 64) and logic depth (longest path through the cone, currently approximated as the number of blocks). These metrics will drive workgroup sizing and load balancing in the multi-cone dispatch implementation.

Currently, skalp generates a single `combinational_cone_0` kernel that evaluates all combinational logic in topological order. The cone extraction infrastructure exists for future multi-cone parallelism, where independent cones would become separate GPU dispatches that can execute concurrently. For most designs, the single-kernel approach is sufficient — the parallelism is within the cone (SIMD evaluation of wide operations) rather than across cones. But for very large designs with independent subsystems, multi-cone dispatch could provide additional speedup by keeping more GPU cores busy.

---

## Lessons and What's Next

**GPU simulation is viable today, not just a research direction.** skalp's GPU backend runs real designs — state machines, FIFOs, pipelined processors — and produces correct results validated against the compiled CPU backend. The engineering is in the details: struct alignment padding (BUG #182), clock edge detection across multiple clock domains (BUG #179, #180), bit-width masking for narrow signals (BUG #181), double-buffering for correct flip-flop semantics, and state element handling for batched mode (BUG #254). None of these are conceptually hard, but each one is a correctness bug that silently produces wrong results if missed. The bug numbers tell the story — getting GPU simulation correct is a long tail of "the 32-bit design works perfectly, but the 48-bit design reads garbage from offset 12 instead of offset 16."

**UMA changes the economics of GPU simulation.** On Apple Silicon, per-step CPU visibility of GPU state is free. This means GPU simulation is viable for interactive debugging, not just batched regression. The testbench can call `set_input()`, `step()`, `get_output()` in a tight loop, and every call is a pointer dereference — no DMA, no staging buffers, no synchronization beyond `wait_until_completed()`. On discrete GPUs, the DMA cost pushes GPU simulation toward batch-only workloads. When the M-series chips can run a 100K-gate design at full per-step visibility with zero DMA overhead, the question shifts from "is GPU simulation fast enough?" to "why would you use the CPU?"

**SharedCodegen is the architectural bet.** Every line of expression generation, struct layout, and evaluation ordering is written once and used by every backend. The Metal backend is 312 lines. The C++ backend is similar. The shared core is 3,200 lines. Adding CUDA is a 300-line wrapper. Adding Vulkan compute would be similar. The investment in the shared core pays compound returns with each new backend, and — critically — every backend gets correctness validation against every other backend for free. Run the same testbench on GPU and CPU, diff the outputs at every cycle, and any divergence points to a runtime bug, not a computation bug.

**Fault simulation is the killer app.** This is where GPU acceleration delivers not just incremental improvement but a change in what's practical. Running 10M fault simulations per second means a complete stuck-at campaign for a 10K-primitive design finishes in 2 seconds instead of minutes. At that speed, fault simulation becomes part of the edit-compile-test loop, not a nightly batch job. Change a safety mechanism, rerun the fault campaign in 2 seconds, see the updated diagnostic coverage immediately. For ISO 26262 safety-critical designs, this means diagnostic coverage numbers are available during development, not discovered after tape-out when fixing them costs 100x more.

**The batched kernel is where the real performance lives.** Per-step simulation on a GPU is bottlenecked by kernel dispatch and synchronization overhead, even with UMA. The batched kernel amortizes that overhead over thousands of cycles — copy state to thread-local, run 10K cycles in a tight loop, write back once. For regression testing where you don't need per-cycle visibility, the batched kernel is the path to maximum throughput. The GPU doesn't need to synchronize with the CPU between cycles, doesn't need to check for stimulus changes, doesn't need to log waveforms — it just computes.

What's next:

- **Multi-cone parallelism.** The cone extraction infrastructure is in place. The next step is generating multiple compute kernels from independent cones and dispatching them concurrently within a single command buffer. For designs with independent subsystems, this could keep more GPU cores active during combinational evaluation.

- **CUDA backend.** SharedCodegen makes this a wrapper exercise. The main work is the runtime — buffer management with `cudaMalloc`/`cudaMemcpy`, kernel launch configuration, and memory strategy selection (explicit DMA for batched, CPU fallback for interactive). The kernel code generation is ~300 lines.

- **Multi-design batched regression.** Instead of simulating one design with one test vector sequence, simulate hundreds of (design, test) pairs in parallel. Each GPU threadgroup runs a different test configuration. This turns the GPU into a regression server — submit a batch of test configurations, get back pass/fail results. For CI/CD pipelines running thousands of tests, this could reduce regression time from hours to minutes.

The infrastructure is in place. The hard part — getting GPU simulation to produce correct, bit-exact results — is done.
