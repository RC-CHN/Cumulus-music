# Full original local stereo material oracle

These fixtures contain synthetic IEEE754 input samples and observed original
SDK output hashes. They distribute no DLL, instruction bytes, original
parameter/state memory or user data. The original interpreter run executed the
hash-pinned x64 3.1.42 SDK constructor 1A050, Init 1AEC0, defaults 1BBA0 and
Process 1AA50. The DLL SHA256 is
`c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be`.

This is a full **inner SDK graph**, including its original disabled children
and first-block transitions. It is not the outer AudioEffects FIFO 67C00/68120,
its input sentinel, the cloudmusic EFB760 JSON material dispatcher, a Windows
runtime, the application UI, a sound-card test or a claim about every preset.

## Frozen observations

- `effects-local-stereo-full-official.json.gz`: 24 cases / 1002 blocks,
  44.1/48/96 kHz × capacity 512/2048 × default all-off, SE1 enabled,
  same-instance SE1 on→off, and original graph seek reset.
- `effects-local-stereo-first-eof-official.json.gz`: 4 cases / 92 blocks,
  44.1/48 kHz, capacity 2048, SE1 enabled, first call 7 valid frames or
  0 valid frames with EOF already set. These include actual full-capacity
  preblend observations for the disabled +68/+78/RVB children.

The exact actual child order at two channels is
`08,18,58,48,50,70,68,60,A0,90,98,28,30,40,78,20,A8,38,B0,10`.
The original +80/+88 slots are absent in these geometries. When the original
zero-frame path skips a converter, the recorded order reflects that omission.
Children are observed at their actual virtual-call entries and return sites;
no trace hook changes control flow, input data, output data, frame counts or
return values. Disabled children are retained; their initial transition must
not be replaced by an assumed no-op.

Input format enum 3 means interleaved f32, two channels. Internal SE format is
0x3102. Master is not bypassed. Original B0 is enabled with threshold 0 dB,
attack f32(0.005), release f32(0.1), output gain 0 dB, infinite=true and ratio 4.
SE1 uses original 1DDC0 with presence 3, stereoizer 5, shaper=true, ambience=1,
then 1DD00(true). The on→off scenario calls only original 1DD00(false) before
block 2, retaining its parameters. The seek scenario calls original 1B960
before block 2, then starts synthetic source frame 8000. It does not rebuild
the graph or imply equivalence to the application's whole-material dispatcher.

Each block records valid source frames, EOF, actions, exact input bits,
observed child sequence and output frame/EOF flow. Final PCM SHA256 covers
**every valid interleaved f32 little-endian sample**. Each child hash covers
**every valid planar left sample followed by every valid planar right sample**.
Selected diagnostic prefixes contain at most 64 frames/channel; they do not
replace full-output hashes. The source remainder up to capacity is explicitly
zeroed. The post-EOF check makes one further original zero-frame/EOF call and
requires `(0,true)` again. All tested outputs are finite.

## Host boundary

Original .text stays read/execute and is checked against the original image
after each case. The original CRT heap, calloc, aligned allocations, free,
security-cookie checker, C-locale initialization, per-thread state and static
guards execute. The exact 520-byte PE TLS template is copied by the test
loader. Required external seams are bounded Windows HeapAlloc/HeapFree, the
SDK allocator vtable, owned-range memory operations, FLS/resolver operations,
single-thread locks, error state and FILETIME. Allocation ownership/families,
16 MiB per allocation, a 64 MiB arena and per-call execution bounds remain
enforced. Unsupported invoked APIs abort the case; exceptions and timeouts
are not converted to DSP success. No host math/DSP callbacks are supplied.

The interpreter starts x87 CW 0x37f and MXCSR 0x1f80. It uses original static
CPU dispatch without running DllMain or the Windows CRT CPU initialization.
This is a declared single-thread host model, not measured native Windows/MSVC
execution. Optional FlsGetValue2 is absent, so the original fallback executes.
Actual NLS reply count is zero in all 28 cases. 44.1/48 kHz runs observe two
original C-locale requests; the 96 kHz cases observe no such request. Retained
en-US NLS adapter definitions are unused and are not evidence of real Windows
locale behavior. No account, network service, device or physical output runs.

## Manual reproduction

The manual wrapper replays the **already frozen exact input bits**, avoiding
new Python/system-libm waveform generation. It passively observes original
outputs and compares every recorded frame count, EOF flag, child order,
full PCM hash, selected bit prefix and first-EOF preblend observation.
Source/case receipts are emitted into a new private output directory; an
existing destination is rejected. Container gzip hashes need not match when
JSON key ordering differs; semantic comparisons remain exact.

Required optional research dependencies are Python 3, `pefile`, `capstone`
and `unicorn`. No dependency is required for `--audit-only`. The original DLL
must be supplied locally by the researcher. The wrapper requires Linux x86_64
and a separate network namespace, re-executing via `unshare -Urn` if necessary.
If unshare is unavailable or forbidden, native execution fails rather than
falling back to an ordinary network namespace. Do not run with Python `-O`.
Never add original-DLL execution to public CI.

```sh
python3 scripts/research/freeze_full_stereo_material.py \
  --fixture path/to/effects-local-stereo-full-official.json.gz \
  --fixture path/to/effects-local-stereo-first-eof-official.json.gz \
  --audit-only

python3 scripts/research/freeze_full_stereo_material.py \
  --fixture path/to/effects-local-stereo-full-official.json.gz \
  --fixture path/to/effects-local-stereo-first-eof-official.json.gz \
  --dll /private/path/neaudioeffects.dll \
  --output-dir /private/new-original-replay
```

The original private generator/adapter hashes are preserved in the two
fixture manifests. The public wrapper is a later packaging of the same host
model; it removes private path and pre-generated disassembly dependencies by
reading the 22 original child callsites from the hash-pinned PE with Capstone.
Its packaging validation currently covers AST parsing, help, corpus shape,
all 28 cases/1094 blocks and hash rejection. **The newly packaged manual
wrapper has not itself been executed against the DLL.** Do not present that
packaging check as a new original SDK run. `effects-local-stereo-oracle-packaging.manifest.json` records
this distinction and the packaged source hashes.

## Additional original control/reset observations

`effects-local-stereo-slider-official.json.gz` adds two actual original flows
(44.1/48 kHz, capacity 2048), 60 blocks total. A fresh all-off first block is
followed by SE1 3/5 on, a live 6/8 control update, off retaining parameters,
2/4 on, pending-off→immediate original 1B960 and pending-on→immediate 1B960.
There is no processing call between each final toggle and reset. Resets start
synthetic cursors 8000 and 16000, and seven valid frames start EOF draining.
Exact ordered actions and observed pre/post-reset flags accompany the samples.

`effects-local-stereo-after-eof-reset-official.json.gz` contains two phases in
**one original instance**, 50 blocks total at 48 kHz/capacity 2048. The first
2055 source frames fully drain to 50055 frames. After original 1B960, 512 new
non-EOF source frames precede seven EOF source frames; these 519 input frames
drain to 48519 frames. Thus this exact new non-EOF sequence receives another
48000-frame tail. This does not show that Reset itself resets a standalone SE
EOF counter: original B6EB0:B702B–B7035 assigns remaining=-1 on non-EOF input.
Reset followed only by empty EOF is a distinct established behavior. No new
reset-counter rule is inferred from the full-graph observation.

The manual replay accepts all four fixture files (32 cases/phases, 1204
blocks), preserving one VM/object/buffer across the two after-EOF phases.
Each extra `--fixture` is explicit; it does not silently widen the old corpus.
The new wrapper remains packaging-only validated, not re-executed against the
DLL. In the repository the packaging manifest is named
`core/tests/fixtures/effects-local-stereo-oracle-packaging.manifest.json`.
