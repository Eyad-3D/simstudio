def f(x):
    return x
def step(t, dt, inputs, state, params):
    if t > 1e9:
        return {'y': f(if=1)}
    return {'y': 2.0}
