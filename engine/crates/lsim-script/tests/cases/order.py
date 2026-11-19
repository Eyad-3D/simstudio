# params: {"z": 1e16, "a": 1.0, "b": 1.0}
def step(t, dt, inputs, state, params):
    s = 0.0
    for v in params.values():
        s += v
    return {'y': s}
