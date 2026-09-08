-- Wrapping +1 per byte over a borrowed region span. n == 0 is a no-op.
-- Pointer is the live region address as USize (not ByteArray).
-- Peek/poke are BaseIO so the compiler cannot delete the in-place stores.

@[extern "lean_w11_load_u8"]
opaque loadUInt8 (p : USize) : BaseIO UInt8

@[extern "lean_w11_store_u8"]
opaque storeUInt8 (p : USize) (v : UInt8) : BaseIO Unit

@[export mutate_lean]
def mutate_lean (p : USize) (n : USize) : BaseIO Unit :=
  let rec go (i : Nat) : BaseIO Unit := do
    if i >= n.toNat then
      pure ()
    else
      let q := p + USize.ofNat i
      let x ← loadUInt8 q
      storeUInt8 q (x + 1)
      go (i + 1)
  go 0
