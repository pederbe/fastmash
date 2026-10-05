// Keep mdBook's sidebar state, resizing and saved preference in one place.
(function () {
  var button = document.getElementById("mdbook-sidebar-toggle");
  var checkbox = document.getElementById("mdbook-sidebar-toggle-anchor");
  if (button && checkbox) {
    button.addEventListener("click", function () { checkbox.click(); });
  }

  // mdBook adds the current page's headings when the document is ready.
  document.addEventListener("DOMContentLoaded", function () {
    document.querySelectorAll(".chapter-fold-toggle").forEach(function (toggle) {
      var item = toggle.closest("li");
      var chapter = toggle.previousElementSibling;
      toggle.setAttribute("role", "button");
      toggle.setAttribute("aria-label", "Toggle sections under " + chapter.textContent.trim());
      function updateExpanded() {
        toggle.setAttribute("aria-expanded", item.classList.contains("expanded"));
      }
      updateExpanded();
      // Heading groups can also expand automatically as the reader scrolls.
      new MutationObserver(updateExpanded).observe(item, {
        attributes: true, attributeFilter: ["class"]
      });
      toggle.addEventListener("keydown", function (event) {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          toggle.click();
        }
      });
    });
    // mdBook's original link list predates its generated heading controls.
    function updateTabStops() {
      var visible = document.documentElement.classList.contains("sidebar-visible");
      document.querySelectorAll("#mdbook-sidebar a").forEach(function (link) {
        link.tabIndex = visible ? 0 : -1;
      });
    }
    updateTabStops();
    new MutationObserver(updateTabStops).observe(document.documentElement, {
      attributes: true, attributeFilter: ["class"]
    });
  });
})();
