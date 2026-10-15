T = {0: {0: 0.0, 1: 1.0}, 1: {0: 0.0, 1: 1.0}}
def step(t, dt, inputs, state, params):
    a = interp(T, 0.5, 0.5)
    T[0][1] = 3.0
    b = interp(T, 0.0, 1.0)
    return {'y': b}
