def step(t, dt, inputs, state, params):
    r = range(-5000000000000000000, 5000000000000000000)
    return {'b': 1.0 if r else 0.0, 'l': len(r)}
