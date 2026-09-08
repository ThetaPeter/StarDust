# CNS graph + Luna Light

Rebuild of Archive CNS11643 / Luna Light on current StarDust: one `WorldLay` region, `CnsGraph` / `LunaStore` BoundClass, Python method bodies (I3). No `.sdb`, no StarDust-written `.sd` programs.

- IR authority: Rust builders in [`tests/cns.rs`](../../tests/cns.rs); copy [`world.sd`](world.sd).
- Data: frozen TSVs in [`data/`](data/) (not a living store).
- Bodies: [`cns.py`](cns.py), [`luna.py`](luna.py). Query character and result strings are header slices.
- UI: right-click [`world.sd`](world.sd) → **Inspect in StarDust** in the cosmos (`cargo run -p fs-viz --features python`). Contract: [`StarDust UI.md`](../../StarDust%20UI.md).

Gates (`cargo test --features python --test cns`):

- `cns_smoke` — TSV table lengths (5428 / 528 / 152 / 16989 / 16648) and checksum **39745**
- `luna_check_store` — Enc/Post cache vs derived mismatch count **0**
- `luna_smoke` — Archive `smokeLuna` unit strings
- `luna_dict_yaml` — derived Post vs committed [`luna_light.dict.yaml`](luna_light.dict.yaml) (Archive Rime gold; overlap with CnsWord; mismatch count **0**)
