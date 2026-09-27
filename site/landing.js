// Fastmash landing page: copy buttons and install tabs.
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
      document.getElementById('copy-status').textContent = 'Command selected; press Ctrl+C to copy';
    };
    if (!navigator.clipboard) { select(); return; }
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
      var step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
      if (step) { event.preventDefault(); select(tabs[(index + step + tabs.length) % tabs.length], true); }
    });
  });
  if (tabs.length) select(tabs[0], false);
})();
