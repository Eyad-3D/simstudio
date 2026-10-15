x = []
for i in range(300000):
    def g(a=x):
        return a
    x = [g]
x = 0
def step(t, dt, inputs, state, params):
    return {'y': 1.0}
