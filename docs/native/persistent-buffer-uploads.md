# Persistent buffer upload packing

## Validation and ownership

`ComputeBatch::import_buffer` imports a persistent destination once per batch.
A new destination requires one complete upload. Initialized destinations accept
empty updates and partial updates; every nonempty range must have four-byte
aligned offset and length, fit the destination without overflow, and be sorted
and disjoint. Reject the complete journal before allocating its payload.

`buffers/packing.rs::Upload` borrows the validated journal. Its byte length and
coalesced descriptor count come from that same immutable borrow, so serializers
cannot pair payloads with independently supplied ranges or destination extents.
Adjacent ranges share a descriptor; gaps never do. Gaps may contain GPU-owned
bytes and must retain their contents.

## Single payload construction

The old common packer extended an initially empty Vec while assembling updates,
allowing repeated growth and copying of large journals. The scatter builder
then copied that payload into a second Vec. The packer now determines sizes first
and allocates the final payload and descriptor capacities once. Ordinary copies
write each source byte once into their owned payload. Metal/DX12 scatter serializes its
header, descriptors and original borrowed slices directly into its final packet;
it does not allocate an intermediate packed payload or descriptor vector.
This removes redundant CPU preparation at its source, without dropping scene
updates or postponing work. It does not solve drawable acquisition waits.

Borrowing ends during import: the batch owns the complete payload before the
caller can modify or release source storage. Validation and allocation failure
semantics remain those of native uploads. Empty journals allocate no payload.

## Backend selection and ordering

Vulkan-only builds retain native copy uploads. Metal and DX12 builds select
scatter for 16–65,535 coalesced copies, with a u32-addressable destination and
complete packet. Small or unaddressable packets retain the copy path. The existing
mixed DX12/Vulkan feature behavior is preserved. The wire format remains four
header words, four words per descriptor and packed source words; each scatter
group owns one disjoint destination range.

Range uploads precede existing compute commands. Every batch owns its source
storage and holds the persistent destination through submission/readback. Receipt
acceptance, full initialization, in-flight isolation, completion and quarantine
rules remain required; packing introduces no new waits or resource-reuse policy.

## Verification

CPU tests cover independent patch results across adjacent and fragmented ranges,
untouched holes, snapshot ownership, empty journals, invalid/overflowing ranges,
packet ABI, upload-before-compute ordering and coalesced scatter thresholds.
Pure packet tests also compile in Metal test builds; this covers serialization
semantics without claiming DX12 hardware validation. The physical GPU regression
covers untouched holes and queued readbacks after dropping the source owner.
Application tail measurements and full SVG/example parity are required separately.

## Application evidence

The maximized AAPL Replay workload (3420×1966, 780 candles) uses alternating
release runs of 1,000 frames each. Sizing alone reduces upload-import CPU time
about 39%, but does not pass diagnostic interval tails. Combined direct-packet
Metal scatter reduces fragmented blit encoding about 94%; across six diagnostic
120 Hz pairs, submit maximum falls 5.3201→4.4653 ms. Native 60 Hz interval maxima
improve in all three pairs. Interval P95 is essentially unchanged; diagnostic
Pmax is 11.8642→12.1601 ms, so this does not deliver an 8.33 ms interval target.
The physical display remains 60 Hz and clocks are unlocked. The combined upload
reduction is retained; drawable phase waits remain a separate unresolved issue.

Release Metal validation has 917 passing tests and the same six failures already
reproduced on unchanged source, with three existing compiler/differential
prerequisites excluded. All 1,713 SVG outputs and both blur qualities are
byte-identical; examples and native-window acceptance pass. Pure packet tests
cover shared DX12 serialization, without a DX12/Vulkan hardware performance claim.
