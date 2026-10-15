# params: {"k": 100000000000000000001}
def step(t, dt, inputs, state, params):
    return {'y': params['k'] % 7}
