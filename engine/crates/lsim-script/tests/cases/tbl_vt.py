# params: {"tbl": {"1\u000b": 1.0, "3": 5.0}}
def step(t, dt, inputs, state, params):
    return {'y': interp(params['tbl'], 2.0)}
