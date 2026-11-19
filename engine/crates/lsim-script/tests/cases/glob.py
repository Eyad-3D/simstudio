G = 1.0
def step(t, dt, inputs, state, params):
    y = G
    global G
    return {'y': y}
