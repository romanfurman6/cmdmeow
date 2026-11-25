// Promo video: "Watch" opens a macOS-style window over the blurred page
(function () {
  var lb = document.getElementById("vlb");
  if (!lb) return;
  var v = lb.querySelector("video"), last = null;
  function open(e) {
    if (e) e.preventDefault();
    last = document.activeElement;
    lb.classList.add("on"); lb.setAttribute("aria-hidden", "false");
    document.documentElement.style.overflow = "hidden";
    v.currentTime = 0; var p = v.play(); if (p && p.catch) p.catch(function () {});
    lb.querySelector(".tl button").focus({ preventScroll: true });
  }
  function close() {
    lb.classList.remove("on"); lb.setAttribute("aria-hidden", "true");
    document.documentElement.style.overflow = "";
    v.pause();
    if (last && last.focus) last.focus({ preventScroll: true });
  }
  document.querySelectorAll("[data-watch]").forEach(function (b) { b.addEventListener("click", open); });
  lb.addEventListener("click", function (e) { if (e.target === lb || e.target.closest(".tl button")) close(); });
  document.addEventListener("keydown", function (e) { if (e.key === "Escape" && lb.classList.contains("on")) close(); });
})();
