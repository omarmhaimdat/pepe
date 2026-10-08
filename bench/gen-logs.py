#!/usr/bin/env python3
"""An nginx access log and error log to time `pepe logs` on.

    bench/gen-logs.py access.log error.log 3      # three days, ending now

Traffic follows the time of day, doubles in the last ten minutes, and has
an outage two hours back; about 650,000 requests a day.
"""
import sys, time, math, random
random.seed(7)
out_access, out_error, days = sys.argv[1], sys.argv[2], float(sys.argv[3])
now = int(time.time()); start = now - int(days*86400)
paths = ["/", "/api/items", "/api/items/%d", "/search?q=%s&page=%d", "/static/app.js", "/static/app.css", "/login", "/api/cart", "/favicon.ico", "/wp-login.php"]
weights = [30, 20, 15, 10, 8, 8, 4, 3, 1, 1]
agents = ["Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36", "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)", "curl/8.4.0", "Googlebot/2.1 (+http://www.google.com/bot.html)", "python-requests/2.31"]
mon = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"]
off = time.localtime().tm_gmtoff
sign = '+' if off >= 0 else '-'; tz = "%s%02d%02d" % (sign, abs(off)//3600, abs(off)%3600//60)
a = open(out_access, "w"); e = open(out_error, "w")
for t in range(start, now+1):
    lt = time.localtime(t)
    hour = lt.tm_hour + lt.tm_min/60
    base = 8 + 7*math.sin((hour-9)/24*2*math.pi)
    if now - t < 600: base *= 2.5     # a surge in the last ten minutes
    bad = 0.25 if (now - 7200 < t < now - 6600) else 0.004   # an outage two hours ago
    n = max(0, int(random.gauss(base, base**0.5)))
    stamp = "%02d/%s/%d:%02d:%02d:%02d %s" % (lt.tm_mday, mon[lt.tm_mon-1], lt.tm_year, lt.tm_hour, lt.tm_min, lt.tm_sec, tz)
    for _ in range(n):
        p = random.choices(paths, weights)[0]
        if "%d" in p and "%s" in p: p = p % (random.choice(["tea","mug","pot"]), random.randint(1,5))
        elif "%d" in p: p = p % random.randint(1, 3000)
        st = 200
        r = random.random()
        if p == "/wp-login.php" or p == "/favicon.ico": st = 404
        elif r < bad: st = 502
        elif r < bad + 0.02: st = 304
        ip = "10.%d.%d.%d" % (random.randint(0,3), random.randint(0,40), random.randint(1,254))
        rt = random.lognormvariate(-3.5, 0.8) * (8 if st == 502 else 1)
        a.write('%s - - [%s] "%s %s HTTP/1.1" %d %d "-" "%s" rt=%.3f urt="%.3f"\n' % (ip, stamp, "POST" if p in ("/login","/api/cart") else "GET", p, st, random.randint(200, 40000), random.choice(agents), rt, rt*0.9))
        if st == 502 and random.random() < 0.5:
            e.write('%d/%02d/%02d %02d:%02d:%02d [error] 31#31: *%d connect() failed (111: Connection refused) while connecting to upstream, client: %s, server: shop.test, request: "GET %s HTTP/1.1", upstream: "http://127.0.0.1:8080%s", host: "shop.test"\n' % (lt.tm_year, lt.tm_mon, lt.tm_mday, lt.tm_hour, lt.tm_min, lt.tm_sec, random.randint(1,99999), ip, p, p))
        if p == "/favicon.ico" and random.random() < 0.3:
            e.write('%d/%02d/%02d %02d:%02d:%02d [error] 31#31: *%d open() "/var/www/html/favicon.ico" failed (2: No such file or directory), client: %s, server: shop.test, request: "GET /favicon.ico HTTP/1.1", host: "shop.test"\n' % (lt.tm_year, lt.tm_mon, lt.tm_mday, lt.tm_hour, lt.tm_min, lt.tm_sec, random.randint(1,99999), ip))
    if t % 3600 == 0:
        e.write('%d/%02d/%02d %02d:%02d:%02d [warn] 31#31: *%d an upstream response is buffered to a temporary file /var/cache/nginx/proxy_temp/1/00/%010d while reading upstream, client: 10.0.0.1, server: shop.test\n' % (lt.tm_year, lt.tm_mon, lt.tm_mday, lt.tm_hour, lt.tm_min, lt.tm_sec, t%1000, t))
