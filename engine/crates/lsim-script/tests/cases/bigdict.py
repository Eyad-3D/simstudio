D = {}
for i in range(200000):
    D[i] = i * 0.5
def step(t, dt, inputs, state, params):
    return {'y': D[199999]}
