Recovered DSP source provenance

netease_dsp.c is copied from references/recovered-client/scripts/netease_dsp.c,
whose SHA256 is 2054c7aa1b352f78a937f551bdb4f490b25c1299f40ef0a9e33f6b41944e0dc0.
It is the previously recovered C implementation, not official DLL machine code
and not recovered original C++ source. Reports 34/35/36 identify native routines,
numerical conventions, test inputs and limits.

The copy extends the three __SSE__ guards to MSVC's _M_X64 / _M_IX86_FP macros
so the same FTZ save/restore is selected there. fast_power10 additionally rejects
non-finite/out-of-int64-range encoded values before its float-to-integer cast;
it returns NAN for the Rust boundary to silence and quarantine the failed stage.
Ordinary inputs preserve the recovered evaluation order and rounding.

netease_rotate.c is an exact copy of the recovered source with SHA256
b4f409b05085893f0a96f4eeb98c83340e275affea7bc061a58c4f36e1753acf.
It implements the 44.1kHz stereo kernel and FIFO only, not rate conversion.
netease_multiband.c is copied from recovered source SHA256
6f547666c8b99f3ec181feaff49f4356455e1facf9ed1286f11f31dad1091f79.
Its byte readers explicitly decode little endian instead of host-endian memcpy.
Reports 37/38 describe these two components and their original recovery limits.

bridge.c adds native sizeof/_Alignof queries, rejects non-finite ratio
reciprocals and preflights all crossover coefficients before state mutation.
That prevents the recovered setter from reading uninitialized coefficients
when a tiny fourth-order frequency causes the coefficient helper to fail.
The Rust API owns allocation, checks format/buffer length, supplies disjoint dry
storage and uses exclusive mutation. No C pointer is public.

build.rs compiles C11 at O2, disallows fast-math/FMA contraction, and uses
/fp:strict for MSVC. Linux x86_64 is actually built and tested. Windows/macOS/ARM
are not yet build/run verified; numerical equivalence is limited to recorded
platform/toolchain/input sets. No official DLL or material is shipped by this
static library. Rotation responses must be supplied and owned by the caller;
Rust retains an immutable Arc for as long as the native state can reference it.
Multiband is the recovered mono component, not the full StereoEnhancer graph.

The Windows CI build uses float literals for exact 0/1 and 120/2000/5000 values
to avoid MSVC C4244 without changing arithmetic. C4324 is suppressed only around
NeteaseMultiband: padding from its explicit 16-byte SIMD alignment is intended,
and Rust obtains native size/alignment through the FFI rather than assuming it.

filter_tangent evaluates a float argument with double tan, then rounds back to
float. A glibc 2.39 versus 2.43 comparison isolated tanf as the sole cause of
2296 differing valid PCM samples in the 102 multiband vectors. The single
substitution restored exact PCM and detector history on both systems; no test
tolerance was widened. See docs/dsp-platform-validation.json.
