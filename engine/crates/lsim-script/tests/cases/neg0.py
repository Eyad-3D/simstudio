T = {0: 0.0, 1: 1.0}
def step(t, dt, inputs, state, params):
    a = interp(T, -1.0)
    T[0] = -0.0
    b = interp(T, -1.0)
    return {'y': b}
