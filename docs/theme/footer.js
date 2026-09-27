// Adds the project footer to every page.
(function () {
  var main = document.querySelector("main");
  if (!main) return;
  var footer = document.createElement("footer");
  footer.className = "fastmash-footer";
  footer.innerHTML =
    'Fastmash is free software under MIT OR Apache-2.0 · ' +
    '<a href="https://github.com/pederbe/fastmash">Source</a> · ' +
    'Created by <a href="https://pederbe.dev" rel="author me">Peder Bergan</a> ' +
    '<span class="fastmash-profiles">' +
    '<a href="https://github.com/pederbe" rel="me">GitHub</a> · ' +
    '<a href="https://www.linkedin.com/in/pederbergan/" rel="me">LinkedIn</a> · ' +
    '<a href="https://bsky.app/profile/pederbe.dev" rel="me">Bluesky</a> · ' +
    'Also by Peder: <a href="https://udpstp.com/">udpstp</a></span>';
  main.appendChild(footer);
})();
