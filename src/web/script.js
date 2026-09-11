(function () {
  var root = document.documentElement;
  var button = document.querySelector('.nav-toggle');
  if (!button) return;

  function remember(collapsed) {
    try {
      localStorage.setItem('flowy-nav', collapsed ? 'closed' : 'open');
    } catch (e) {
      // Private browsing and file:// origins can refuse storage; the toggle
      // still works for the life of the page.
    }
  }

  function sync() {
    var collapsed = root.classList.contains('nav-collapsed');
    button.setAttribute('aria-expanded', String(!collapsed));
    button.title = collapsed ? 'Show workstreams' : 'Hide workstreams';
  }

  button.addEventListener('click', function () {
    remember(root.classList.toggle('nav-collapsed'));
    sync();
  });

  sync();
})();
