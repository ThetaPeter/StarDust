def mutate(buf):
    for i in range(len(buf)):
        buf[i] = (buf[i] + 1) & 0xFF
