def step(t, dt, inputs, state, params):
    n = 60000
    d = dict(list(zip(range(n), range(n))))
    return {'y': d[n - 1] + len(d)}
