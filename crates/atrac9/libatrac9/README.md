# LibAtrac9 (vendored)

The C decoder of [LibAtrac9](https://github.com/Thealexbarney/LibAtrac9) by Alex Barney, MIT licence (`LICENSE` in this
folder), copied from `C/src` at commit `efca2e3af35562a09a9bb6deed90e45b4b824dc4` (2025-03-14). It is the
reference ATRAC9 decoder and supports band extension, which the theme songs of PS5 games use at low bit rates.

`../build.rs` compiles it into a static library; `../src/lib.rs` is the only code that calls it.

## Our changes

Upstream decodes valid streams but trusts every value it reads, so a damaged file indexes past its tables. Fuzzing with
AddressSanitizer and UndefinedBehaviorSanitizer (`../fuzz/noise.c`) found the places; each fix is a bounds check marked
`AnyPlay:` in the code, and none changes how a valid stream decodes:

- `unpack.c`: band extension needs 13 to 20 quantization units (`BexGroupInfo` has 8 entries), checked where the band
  parameters are read and again where reused ones are used; a missing Huffman codebook is an error, not a null read.
- `band_extension.c`: every table index (mode, values, scale factors, unit count) is checked before the extension runs.
- `bit_allocation.c`: the gradient fits its 31 entries; the fine precision stays within its 16-entry table.
- `utility.c`: no 32-bit shift by 32, no left shift of a negative value.

After the fixes: one million random superframes over five configurations, no finding. Rerun the fuzzer after any update:
`gcc -fsanitize=address,undefined -g -O1 -I. *.c ../fuzz/noise.c -lm -o noise && ./noise 200000` (on Linux).

To update, copy the newer `C/src` here, reapply the changes marked `AnyPlay:`, keep this file and `LICENSE`, note the new
commit and fuzz again.
