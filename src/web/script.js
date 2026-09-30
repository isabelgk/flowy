(function () {
  var root = document.documentElement;
  var button = document.querySelector('.nav-toggle');
  var nav = document.getElementById('workstreams');
  if (!button || !nav) return;

  // A cookie rather than localStorage, so the server knows whether to spend a
  // `git status` per worktree on the sidebar.
  function remember(collapsed) {
    document.cookie = 'flowy-nav=' + (collapsed ? 'closed' : 'open') +
      '; path=/; max-age=31536000; SameSite=Strict';
  }

  // A page rendered with the sidebar closed has an empty one to fill in.
  function load() {
    if (nav.firstElementChild) return;
    var url = '/nav';
    if (nav.dataset.active) url += '?w=' + encodeURIComponent(nav.dataset.active);
    fetch(url).then(function (response) {
      return response.ok ? response.text() : '';
    }).then(function (html) {
      if (!nav.firstElementChild) nav.innerHTML = html;
    });
  }

  function sync() {
    var collapsed = root.classList.contains('nav-collapsed');
    button.setAttribute('aria-expanded', String(!collapsed));
    button.title = collapsed ? 'Show workstreams' : 'Hide workstreams';
  }

  button.addEventListener('click', function () {
    var collapsed = root.classList.toggle('nav-collapsed');
    remember(collapsed);
    if (!collapsed) load();
    sync();
  });

  sync();
})();
