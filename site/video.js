// Promo video: "Watch" opens a macOS-style window over the blurred page
(function () {
  var lb = document.getElementById("vlb");
  if (!lb) return;
  var v = lb.querySelector("video"), last = null;
  // keep keyboard focus inside the window while it is open
  function bg(on) { document.querySelectorAll("body > :not(#vlb)").forEach(function (el) { el.inert = on; }); }
  function open(e) {
    if (e) e.preventDefault();
    last = document.activeElement;
    lb.classList.add("on"); lb.setAttribute("aria-hidden", "false");
    document.documentElement.style.overflow = "hidden";
    bg(true);
    v.currentTime = 0; var p = v.play(); if (p && p.catch) p.catch(function () {});
    requestAnimationFrame(function () { lb.querySelector(".tl button").focus({ preventScroll: true }); });
  }
  function close() {
    lb.classList.remove("on"); lb.setAttribute("aria-hidden", "true");
    document.documentElement.style.overflow = "";
    bg(false);
    v.pause();
    if (last && last.focus) last.focus({ preventScroll: true });
    if (location.hash === "#watch") history.replaceState(null, "", location.pathname + location.search);
  }
  document.querySelectorAll("[data-watch]").forEach(function (b) { b.addEventListener("click", open); });
  lb.addEventListener("click", function (e) { if (e.target === lb || e.target.closest(".tl button")) close(); });
  // cmdmeow.com/#watch (README link) opens the window right away
  if (location.hash === "#watch") open();
  window.addEventListener("hashchange", function () { if (location.hash === "#watch" && !lb.classList.contains("on")) open(); });
  window.addEventListener("keydown", function (e) { if (e.key === "Escape" && lb.classList.contains("on")) { e.preventDefault(); close(); } }, true);
})();
