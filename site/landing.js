// Fastmash landing page: copy buttons, install tabs and decorative motion.
// This head script prepares entrances before the body can paint. With JavaScript
// disabled, unsupported animation APIs, or reduced motion, the HTML stays visible.
(function () {
  if (window.matchMedia && Element.prototype.animate && 'IntersectionObserver' in window &&
      !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    document.documentElement.setAttribute('data-entrances-ready', '');
  }
  document.addEventListener('DOMContentLoaded', function () {

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
// Install tabs: HTML selects the first panel before this script runs.
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

// Decorative motion runs only in view. All content remains usable without it.
(function () {
  var button = document.getElementById('motion-toggle');
  if (!button || !window.matchMedia || !Element.prototype.animate || !('IntersectionObserver' in window)) return;
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
  var requested = true;
  var effects = new Map();
  var entrances = new Map();
  var revealTargets = Array.prototype.slice.call(document.querySelectorAll('[data-reveal]'));
  function pause(animation) {
    var time = animation.currentTime;
    animation.pause();
    if (time !== null) animation.currentTime = time;
  }
  function finishEntrance(element) {
    entranceObserver.unobserve(element);
    element.setAttribute('data-revealed', '');
    var animation = entrances.get(element);
    if (animation) animation.finish();
  }
  // Breathing glows animate here. Auras animate in CSS and follow data-motion-active.
  document.querySelectorAll('[data-motion], [data-aura]').forEach(function (element) {
    var animation;
    if (element.dataset.motion === 'breathe') {
      var opacity = Number(getComputedStyle(element).opacity);
      animation = element.animate([
        { opacity: opacity * 0.6, transform: 'scale(0.96)' },
        { opacity: opacity, transform: 'scale(1.05)' },
        { opacity: opacity * 0.6, transform: 'scale(0.96)' }
      ], { duration: 7000, iterations: Infinity, easing: 'ease-in-out' });
      animation.pause();
    }
    effects.set(element, { animation: animation, visible: false });
  });
  function update() {
    var paused = !requested || reduced.matches;
    effects.forEach(function (effect, element) {
      var running = !paused && !document.hidden && effect.visible;
      element.toggleAttribute('data-motion-active', running);
      if (effect.animation) {
        if (reduced.matches) effect.animation.cancel();
        else if (running) effect.animation.play();
        else pause(effect.animation);
      }
    });
    if (paused) revealTargets.forEach(finishEntrance);
    else entrances.forEach(function (animation) {
      if (document.hidden) pause(animation);
      else if (animation.playState === 'paused') animation.play();
    });
    button.disabled = reduced.matches;
    button.textContent = reduced.matches ? 'Motion off' : requested ? 'Pause motion' : 'Resume motion';
  }
  var observer = new IntersectionObserver(function (entries) {
    entries.forEach(function (entry) {
      effects.get(entry.target).visible = entry.isIntersecting;
    });
    update();
  }, { threshold: 0.1 });
  // Content rises into place once it is clearly in view, in a short sequence.
  var entranceObserver = new IntersectionObserver(function (entries) {
    var delay = 0;
    entries.forEach(function (entry) {
      if (!entry.isIntersecting || entry.intersectionRatio < 0.05) return;
      var element = entry.target;
      entranceObserver.unobserve(element);
      if (requested && !reduced.matches) {
        var animation = element.animate([
          { opacity: 0, transform: 'translateY(48px)' },
          { opacity: 1, transform: 'translateY(0)' }
        ], { duration: 1200, delay: delay, easing: 'cubic-bezier(.25,.1,.25,1)', fill: 'backwards' });
        entrances.set(element, animation);
        animation.onfinish = function () { entrances.delete(element); };
        delay = Math.min(delay + 140, 420);
      }
      element.setAttribute('data-revealed', '');
    });
    update();
  }, { rootMargin: '0px 0px -64px 0px', threshold: 0.05 });
  effects.forEach(function (_, element) { observer.observe(element); });
  revealTargets.forEach(function (element) { entranceObserver.observe(element); });
  // Keyboard focus never waits for an entrance. A running entrance finishes only
  // for keyboard focus, so a mouse press does not move its target mid-click.
  document.addEventListener('focusin', function (event) {
    revealTargets.forEach(function (element) {
      if (!element.contains(event.target)) return;
      if (!element.hasAttribute('data-revealed') ||
          (entrances.has(element) && event.target.matches(':focus-visible'))) finishEntrance(element);
    });
  });
  button.addEventListener('click', function () { requested = !requested; update(); });
  reduced.addEventListener('change', update);
  document.addEventListener('visibilitychange', update);
  button.hidden = false;
  update();
})();

  });
})();
