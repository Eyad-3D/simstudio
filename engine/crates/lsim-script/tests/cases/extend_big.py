def step(t, dt, inputs, state, params):
    inner = list(range(1000))
    big = [inner]
    for i in range(10):
        big.extend(big)
    return {'y': len(big)}
