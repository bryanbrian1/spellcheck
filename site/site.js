// Two conveniences, neither required: the page reads fully without them.
(function () {
  // 1. Put the visitor's own platform first. The other button stays one
  //    press away, just quieter; a Windows player on a Mac at work still
  //    gets both. The stylesheet already makes the second button the quiet
  //    one, so with no script, or no match, the page reads "mac, then
  //    Windows" rather than two buttons shouting.
  var ua = navigator.userAgent || "";
  var phone = /Android|iPhone|iPad|iPod/.test(ua) ||
    (window.matchMedia && window.matchMedia("(hover: none) and (pointer: coarse)").matches);
  var os = phone ? null : /Windows/.test(ua) ? "win" : /Mac/.test(ua) ? "mac" : null;
  if (os) {
    document.querySelectorAll(".installs").forEach(function (group) {
      var mine = group.querySelector('.install[data-os="' + os + '"]');
      if (!mine) return;
      group.querySelectorAll(".install").forEach(function (b) {
        if (b !== mine) b.classList.add("other");
      });
      group.insertBefore(mine, group.firstChild);
    });
  }

  // A phone can't install a desktop app. Its job is to remember the link,
  // so say so and offer to copy it.
  if (phone) {
    document.querySelectorAll(".fine.later").forEach(function (p) { p.hidden = false; });
  }
  document.querySelectorAll(".copy[data-copy]").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var url = btn.getAttribute("data-copy");
      var done = function () {
        btn.textContent = "Copied";
        btn.setAttribute("data-done", "");
      };
      var fallback = function () {
        // No clipboard access: say the address instead, in a sentence
        // that still reads once the button is gone.
        var p = btn.closest("p");
        var span = document.createElement("span");
        span.className = "mono";
        span.textContent = url.replace(/^https?:\/\//, "").replace(/\/$/, "");
        p.textContent = "spellcheck is a desktop app for macOS and Windows. The address is ";
        p.appendChild(span);
        p.appendChild(document.createTextNode("."));
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(url).then(done, fallback);
      } else {
        fallback();
      }
    });
  });

  // 2. Tags open on a tap as well as hover and focus; a phone has neither.
  var tags = Array.prototype.slice.call(document.querySelectorAll(".tag[aria-describedby]"));
  function closeTags(except) {
    tags.forEach(function (t) { if (t !== except) t.setAttribute("aria-expanded", "false"); });
  }
  tags.forEach(function (t) {
    t.setAttribute("aria-expanded", "false");
    t.addEventListener("click", function (e) {
      e.stopPropagation();
      var open = t.getAttribute("aria-expanded") === "true";
      closeTags(t);
      t.setAttribute("aria-expanded", open ? "false" : "true");
    });
  });
  document.addEventListener("click", function () { closeTags(null); });
  document.addEventListener("keydown", function (e) { if (e.key === "Escape") closeTags(null); });

  // 3. Mark the section in view in the index: the last heading that has
  //    crossed a line a little below the top of the viewport.
  var links = Array.prototype.slice.call(document.querySelectorAll('.index ol a[href^="#"]'));
  var sections = links.map(function (a) { return document.getElementById(a.getAttribute("href").slice(1)); }).filter(Boolean);
  if (!sections.length) return;
  var ticking = false;
  function update() {
    ticking = false;
    var line = Math.min(160, window.innerHeight * 0.25);
    var current = null;
    for (var i = 0; i < sections.length; i++) {
      if (sections[i].getBoundingClientRect().top <= line) current = sections[i].id;
    }
    // At the very bottom the last section may never reach the line.
    if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2) current = sections[sections.length - 1].id;
    links.forEach(function (a) {
      if (current && a.getAttribute("href") === "#" + current) a.setAttribute("aria-current", "true");
      else a.removeAttribute("aria-current");
    });
  }
  function onScroll() { if (!ticking) { ticking = true; requestAnimationFrame(update); } }
  window.addEventListener("scroll", onScroll, { passive: true });
  window.addEventListener("resize", onScroll);
  update();
})();
