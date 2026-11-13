Library to read & write Dolby Vision metadata.  
Comes as a Rust crate and C compatible library.  

See [changelog](CHANGELOG.md) for API changes.

### Mapping API migration

Curves expose ordered `DoviMappingPiece::Polynomial` and `DoviMappingPiece::MMR`
values. Each piece owns its coefficients, including its signed integer when the header
uses fixed-point encoding. Iterate `curve.pieces`; do not assume one method per component.
Pivots retain coded deltas. `validate_with_header` checks complete mapping consistency
before writing or C export, and `write` performs that check before emitting mapping bits.
Unsupported polynomial interpolation returns an error. Errors are `anyhow::Error` values,
not a stable typed classification API.

JSON output and the C mapping ABI are breaking changes described in the changelog.
The v2 C getter returns a separate owner: inspect its per-piece tag/counts and free it once
with `dovi_rpu_free_data_mapping_v2`. Do not modify exported fields or reuse borrowed
pointers after freeing the mapping. `examples/mapping_v2_fixture.rs` authors mixed curves;
`examples/mapping_v2_test.c` checks them through the generated C header and versioned API.

&nbsp;

### Toolchain

The minimum Rust version to use `dolby_vision` is 1.88.0.

&nbsp;

### `libdovi`, C-API

Packages
- **Arch Linux**: available on the AUR, `libdovi` or `libdovi-git`.

&nbsp;

#### Building the library

`libdovi` comes as a C compatible library.  
To build and install it you can use [cargo-c](https://crates.io/crates/cargo-c):

```sh
cargo install cargo-c
cargo cinstall --release
```

#### Running the C-API example
```sh
cd examples
gcc capi_rpu_file.c -ldovi -o capi_example.o
./capi_example.o
```
