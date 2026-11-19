def step(t, dt, inputs, state, params):
    a = [0.0]
    for i in range(80):
        a = [a, a]
    return {'y': 1.0 if a in [1] else 0.0}
