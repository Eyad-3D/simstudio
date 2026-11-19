def step(t, dt, inputs, state, params):
    d = {'a': 1.0, 'b': 2.0, 'c': 3.0}
    s = 0.0
    n = 0
    for k in d:
        n += 1
        s = s * 10 + d[k]
        if k == 'a':
            d.pop('a')
            d['a'] = 9.0
    return {'s': s, 'n': n}
