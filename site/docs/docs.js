// The docs' few behaviours: the theme switch, the phone menu, copy buttons
// on code, the search box, and the "on this page" list following the scroll
(function () {
  var root = document.documentElement;

  // Theme: the switch cycles light and dark and remembers the choice
  var theme = document.querySelector('.theme');
  if (theme) {
    theme.addEventListener('click', function () {
      var dark = root.dataset.theme === 'dark' ||
        (!root.dataset.theme && window.matchMedia('(prefers-color-scheme: dark)').matches);
      var next = dark ? 'light' : 'dark';
      root.dataset.theme = next;
      try { localStorage.setItem('pepe-theme', next); } catch (e) {}
    });
  }

  // The phone menu
  var menu = document.querySelector('.menu');
  var side = document.getElementById('side');
  if (menu && side) {
    menu.addEventListener('click', function () {
      var open = side.classList.toggle('open');
      menu.setAttribute('aria-expanded', open ? 'true' : 'false');
    });
  }

  // Copy buttons
  document.querySelectorAll('.code').forEach(function (block) {
    var button = block.querySelector('.copy');
    var code = block.querySelector('pre');
    if (!button || !code) return;
    button.addEventListener('click', function () {
      var text = code.textContent;
      var done = function () {
        button.textContent = 'copied';
        button.classList.add('done');
        setTimeout(function () { button.textContent = 'copy'; button.classList.remove('done'); }, 1500);
      };
      if (navigator.clipboard) navigator.clipboard.writeText(text).then(done, done);
      else done();
    });
  });

  // Search: the index is fetched when the box is first used
  var input = document.querySelector('.search input');
  var results = document.querySelector('.results');
  var index = null, picked = -1;
  function load() {
    if (index) return Promise.resolve(index);
    return fetch('search.json').then(function (r) { return r.json(); }).then(function (j) { index = j; return j; });
  }
  function score(entry, words) {
    var title = (entry.n + ' ' + entry.t).toLowerCase();
    var text = entry.x.toLowerCase();
    var s = 0;
    for (var i = 0; i < words.length; i++) {
      var w = words[i];
      if (title.indexOf(w) >= 0) s += 10;
      else if (text.indexOf(w) >= 0) s += 2;
      else return 0;
    }
    return s;
  }
  function show(list, words) {
    results.innerHTML = '';
    picked = -1;
    if (!list.length) {
      results.innerHTML = '<div class="none">Nothing for that. Try a flag, a mode or a word from a heading.</div>';
      results.hidden = false;
      return;
    }
    list.slice(0, 12).forEach(function (e) {
      var a = document.createElement('a');
      a.href = e.p + (e.a ? '#' + e.a : '');
      var b = document.createElement('b');
      b.textContent = e.t || e.n;
      var small = document.createElement('small');
      small.textContent = e.t ? e.n : '';
      b.appendChild(small);
      var span = document.createElement('span');
      var at = -1;
      for (var i = 0; i < words.length && at < 0; i++) at = e.x.toLowerCase().indexOf(words[i]);
      span.textContent = at > 40 ? '…' + e.x.slice(at - 40) : e.x;
      a.appendChild(b);
      a.appendChild(span);
      results.appendChild(a);
    });
    results.hidden = false;
  }
  function search() {
    var q = input.value.trim().toLowerCase();
    if (!q) { results.hidden = true; return; }
    var words = q.split(/\s+/);
    load().then(function (list) {
      var found = list.map(function (e) { return { e: e, s: score(e, words) }; })
        .filter(function (x) { return x.s > 0; })
        .sort(function (a, b) { return b.s - a.s; })
        .map(function (x) { return x.e; });
      show(found, words);
    });
  }
  if (input && results) {
    input.addEventListener('input', search);
    input.addEventListener('focus', function () { load(); if (input.value) search(); });
    input.addEventListener('keydown', function (e) {
      var items = results.querySelectorAll('a');
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (!items.length) return;
        picked = (picked + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
        items.forEach(function (a, i) { a.classList.toggle('on', i === picked); });
        items[picked].scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter' && picked >= 0 && items[picked]) {
        window.location.href = items[picked].href;
      } else if (e.key === 'Escape') {
        results.hidden = true;
        input.blur();
      }
    });
    document.addEventListener('click', function (e) {
      if (!e.target.closest('.search')) results.hidden = true;
    });
    document.addEventListener('keydown', function (e) {
      if (e.key === '/' && document.activeElement !== input && !/input|textarea/i.test(document.activeElement.tagName)) {
        e.preventDefault();
        input.focus();
      }
    });
  }

  // The "on this page" list follows the heading in view
  var links = Array.prototype.slice.call(document.querySelectorAll('.toc a'));
  if (links.length && 'IntersectionObserver' in window) {
    var byId = {};
    links.forEach(function (a) { byId[a.getAttribute('href').slice(1)] = a; });
    var current = null;
    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          if (current) current.classList.remove('on');
          current = byId[entry.target.id];
          if (current) current.classList.add('on');
        }
      });
    }, { rootMargin: '-70px 0px -70% 0px' });
    Object.keys(byId).forEach(function (id) {
      var h = document.getElementById(id);
      if (h) observer.observe(h);
    });
  }
})();
