# Frame.explain: writable memoryview of the declared class span.
# kind is u16 LE at 0; note is 32 bytes at 8.
def explain(buf):
    kind = int.from_bytes(buf[0:2], "little")
    names = {
        0x0800: b"IPv4",
        0x86DD: b"IPv6",
        0x0806: b"ARP",
    }
    text = names.get(kind, b"UNK")
    note = bytearray(32)
    note[: len(text)] = text
    buf[8:40] = note
