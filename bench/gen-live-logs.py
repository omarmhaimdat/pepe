#!/usr/bin/env python3
"""An nginx access log and error log written live, for `pepe logs --serve`.

    bench/gen-live-logs.py DIR 400        # about 400 requests a second

Writes thirty minutes of history first, so the page has a usual minute to
hold now against, then a second's lines each second, with an outage of
502s now and then (and their error log lines).
"""
import random, sys, time, math, os
d = sys.argv[1]; rate = int(sys.argv[2]) if len(sys.argv) > 2 else 400
A = open(os.path.join(d, 'access.log'), 'a', buffering=1 << 16)
E = open(os.path.join(d, 'error.log'), 'a', buffering=1 << 12)
P = [('/api/checkout', 30, 'POST'), ('/api/cart', 22, 'GET'), ('/api/products/%d', 60, 'GET'), ('/api/search', 25, 'GET'),
     ('/health', 12, 'GET'), ('/static/app.js', 40, 'GET'), ('/api/login', 6, 'POST'), ('/api/orders/%d', 9, 'GET'), ('/img/hero-%d.webp', 35, 'GET')]
W = [p[1] for p in P]; IPS = ['203.0.113.%d' % i for i in range(1, 60)]
M = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec']
def stamp(t):
    g = time.gmtime(t); return '%02d/%s/%d:%02d:%02d:%02d +0000' % (g.tm_mday, M[g.tm_mon-1], g.tm_year, g.tm_hour, g.tm_min, g.tm_sec)
def estamp(t):
    g = time.gmtime(t - time.timezone if time.daylight == 0 else t - time.altzone); return time.strftime('%Y/%m/%d %H:%M:%S', time.localtime(t))
def second(t, n, outage):
    out = []
    for _ in range(n):
        i = random.choices(range(len(P)), W)[0]; path, _, m = P[i]
        if '%d' in path: path = path % random.randint(1, 5000)
        s = 200
        if outage and i in (0, 7) and random.random() < .6: s = 502
        elif random.random() < .02: s = 404
        elif random.random() < .08: s = 304
        rt = random.expovariate(1 / (0.02 + i * .004)) if s != 502 else random.uniform(3, 30)
        if i == 3: rt *= 2.2
        out.append('%s - - [%s] "%s %s HTTP/1.1" %d %d "-" "Mozilla/5.0" rt=%.3f urt="%.3f"\n' % (random.choice(IPS), stamp(t), m, path, s, random.randint(200, 9000), rt, rt * .9))
        if s == 502:
            E.write('%s [error] 811#811: *%d upstream timed out (110: Connection timed out) while reading response header from upstream, client: %s, server: shop.test, request: "%s %s HTTP/1.1", upstream: "http://10.0.3.12:8080%s", host: "shop.test"\n' % (estamp(t), random.randint(1, 10**6), random.choice(IPS), m, path, path))
    A.write(''.join(out))
now = time.time()
for k in range(1800, 0, -1):
    t = now - k; second(int(t), int(rate * (0.6 + 0.3 * math.sin(k / 400))), False)
A.flush(); E.flush()
t0 = time.time(); outage = 0
while True:
    t = time.time()
    if random.random() < .01: outage = 30
    n = int(rate * (1 + .2 * math.sin(t / 40)) * random.uniform(.9, 1.1))
    second(int(t), n, outage > 0); outage = max(0, outage - 1)
    if random.random() < .05: E.write('%s [warn] 811#811: *%d an upstream response is buffered to a temporary file /var/cache/nginx/proxy_temp/1/00/0000000001 while reading upstream\n' % (estamp(t), random.randint(1, 10**6)))
    A.flush(); E.flush()
    time.sleep(max(0, 1 - (time.time() - t)))
