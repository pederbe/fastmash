// Fastmash landing page: copy buttons, install tabs and one-time motion.
// Copy buttons.
document.querySelectorAll('button[data-copy]').forEach(function (button) {
  button.addEventListener('click', function () {
    var code = document.getElementById(button.dataset.copy);
    // Without clipboard access, select the command so it can be copied by hand.
    var select = function () {
      var range = document.createRange();
      range.selectNodeContents(code);
      var selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.getElementById('copy-status').textContent = 'Command selected; press Ctrl+C, or Command+C on a Mac, to copy it';
    };
    if (!navigator.clipboard || !window.isSecureContext) { select(); return; }
    navigator.clipboard.writeText(code.textContent).then(function () {
      button.textContent = 'Copied';
      document.getElementById('copy-status').textContent = 'Copied to the clipboard';
      setTimeout(function () { button.textContent = 'Copy'; }, 1600);
    }, select);
  });
});
// Install tabs: without JavaScript every option stays visible.
(function () {
  var tabs = Array.prototype.slice.call(document.querySelectorAll('.install-tabs [role="tab"]'));
  function select(tab, focus) {
    tabs.forEach(function (other) {
      var chosen = other === tab;
      other.setAttribute('aria-selected', chosen ? 'true' : 'false');
      other.tabIndex = chosen ? 0 : -1;
      document.getElementById(other.getAttribute('aria-controls')).hidden = !chosen;
    });
    if (focus) tab.focus();
  }
  tabs.forEach(function (tab, index) {
    tab.addEventListener('click', function () { select(tab, false); });
    tab.addEventListener('keydown', function (event) {
      var next = null;
      if (event.key === 'ArrowRight') next = tabs[(index + 1) % tabs.length];
      else if (event.key === 'ArrowLeft') next = tabs[(index - 1 + tabs.length) % tabs.length];
      else if (event.key === 'Home') next = tabs[0];
      else if (event.key === 'End') next = tabs[tabs.length - 1];
      if (next) { event.preventDefault(); select(next, true); }
    });
  });
  if (tabs.length) select(tabs[0], false);
})();

// Motion never hides content or determines whether a control works.
(function () {
  var reduced = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)');
  if ((reduced && reduced.matches) || !('IntersectionObserver' in window)) return;
  var loaded = new Promise(function (resolve) {
    if (document.readyState === 'complete') resolve();
    else window.addEventListener('load', resolve, { once: true });
  });
  var fonts = document.fonts ? document.fonts.ready : Promise.resolve();
  Promise.all([loaded, fonts]).then(function () {
    var root = document.documentElement;
    var states = Array.prototype.map.call(document.querySelectorAll('.hero, .hero-copy, .terminal, .workflow-card, .switch-copy'), function (element) {
      return { element: element, visible: false, timer: null };
    });
    function cancel(state) { clearTimeout(state.timer); state.timer = null; }
    function schedule(state) {
      if (!state.visible || state.timer !== null || state.element.classList.contains('is-revealed') || document.visibilityState === 'hidden' || (reduced && reduced.matches)) return;
      state.timer = setTimeout(function () {
        state.timer = null;
        if (!state.visible || document.visibilityState === 'hidden' || (reduced && reduced.matches)) return;
        state.element.classList.add('is-revealed');
        observer.unobserve(state.element);
      }, 400);
    }
    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        var state = states.find(function (candidate) { return candidate.element === entry.target; });
        state.visible = entry.isIntersecting && entry.intersectionRatio >= 0.18;
        if (state.visible) schedule(state);
        else cancel(state);
      });
    }, { threshold: 0.18 });
    function visibilityChanged() {
      root.classList.toggle('landing-paused', document.visibilityState === 'hidden');
      states.forEach(function (state) {
        if (document.visibilityState === 'hidden') cancel(state);
        else schedule(state);
      });
    }
    document.addEventListener('visibilitychange', visibilityChanged);
    visibilityChanged();
    states.forEach(function (state) { observer.observe(state.element); });
  }).catch(function (error) { console.error(error); });
})();
