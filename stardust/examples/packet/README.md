# Packet workshop

Prefix inheritance, mixed-language methods, and a C export link on one region.

- IR authority: Rust builder in [`tests/packet.rs`](../../tests/packet.rs); copy [`frame.sd`](frame.sd).
- `Frame.parse` is C (`parse.c`); `Frame.explain` is Python ([`explain.py`](explain.py)) and writes into `note`.
- `EthFrame` prefixes `FrameLay` with `dst` / `src` and overrides `parse` in Rust ([`parse_eth.rs`](parse_eth.rs)). A `Frame` view of the same offset still runs the C parser on the base length.
- `ProtocolDb` is linked at `proto`. `>> seal` is C and writes `digest` from the table span only.

MAC addresses sit after the base header so the derived layout is a struct prefix.

Gate (`cargo test --features python --test packet`):

- `packet_declared_dispatch` — C then Python then Rust share `as_ptr()`; Rust checksum includes the MAC bytes; a `Frame` view does not pick up that override; `seal` changes only the protocol table.
