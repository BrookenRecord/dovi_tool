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

### Bounded HEVC parsing

`DoviRpu::parse_unspec62_nalu_bounded(&[u8])` accepts at most 8192 input bytes,
including framing and emulation-prevention bytes, and checks this limit before
allocating. It returns an owned RPU and a fixed-storage `BoundedParseError` on
failure. The error occupies at most 256 bytes, retains a 192-byte UTF-8 message
prefix and four context tags, and never constructs Anyhow errors or backtraces.
Categories distinguish invalid input, unsupported syntax, capacity refusal and
slice-reader I/O errors. I/O errors retain their `ErrorKind`, discarding their
original source and message.

Legacy parsing and validation APIs retain Anyhow results and their uncapped input
behavior through the same parsing bodies. Exact error text and error-chain
identity are unspecified; legacy reader errors remain downcastable to
`std::io::Error` with the original kind. Mapping ownership, coefficient values,
serde output and the C ABI are unchanged by this entry point.

The fixed error storage bounds diagnostics, not process memory. EPB removal and
retained-tail storage still use infallible allocations; system OOM is not a
recoverable parse error. The retained tail reserves its actual remaining input
bits once. Mapping pieces and extension blocks have compile-time layout ceilings
of 1024 and 256 bytes. A consuming application must qualify its toolchain and
target allocation requests, admit bounded input before copying, and account for
retained and concurrent objects separately. This library does not implement an
application metadata pool or establish a process RSS limit.
