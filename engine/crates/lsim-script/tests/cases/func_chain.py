g = 0
for i in range(300000):
    def g(a=g):
        return a
g = 0
def step(t, dt, inputs, state, params):
    return {'y': 1.0}
