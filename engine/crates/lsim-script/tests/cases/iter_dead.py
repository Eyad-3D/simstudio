def step(t, dt, inputs, state, params):
    l = [1.0]
    e = enumerate(l)
    for x in e:
        pass
    l.append(2.0)
    l.append(3.0)
    s = 0.0
    for i, x in e:
        s += x
    m = [1.0]
    z = zip(m)
    a = len(list(z))
    m.append(2.0)
    b = len(list(z))
    d = {'a': 1.0}
    f = enumerate(d)
    for x in f:
        pass
    d['b'] = 2.0
    c = 0.0
    for x in f:
        c += 1.0
    return {'s': s, 'a': a, 'b': b, 'c': c}
