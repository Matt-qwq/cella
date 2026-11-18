/* ============================================================
   CELLA — "Precision Blueprint" site behaviour.
   Progressive enhancement only: with JavaScript disabled, every
   fact on the page is already readable. No frameworks, no build.
   ============================================================ */
(function () {
  "use strict";

  var doc = document.documentElement;
  doc.classList.add("js");

  var mQuery = window.matchMedia;
  var reduceMotion = mQuery && mQuery("(prefers-reduced-motion: reduce)").matches;
  var finePointer = mQuery && mQuery("(pointer: fine)").matches;

  /* -------------------------------------------------- the sheet chrome */
  /* The page behaves like graph paper: a fixed frame, column letters
     along the top, row numbers down the left that climb as you scroll,
     and an active-cell crosshair that tracks the pointer. The readout
     in the nav is the Name Box. All of it is decorative, and all of it
     is disabled under prefers-reduced-motion and on coarse pointers. */

  var ROW_H = 48;               /* must match --rowh in style.css */
  var MIN_COL_W = 90;           /* px per ruler column */
  var framePx = 10;             /* --frame inset, re-read on resize */

  function colName(i) {
    var s = "";
    i += 1;
    while (i > 0) {
      var r = (i - 1) % 26;
      s = String.fromCharCode(65 + r) + s;
      i = Math.floor((i - 1) / 26);
    }
    return s;
  }

  var rulerX = document.getElementById("rulerX");
  var rulerY = document.getElementById("rulerY");
  var chV = document.getElementById("chV");
  var chH = document.getElementById("chH");
  var chM = document.getElementById("chM");
  var navRef = document.getElementById("navRef");

  var wantChrome = !reduceMotion && finePointer && rulerX && rulerY;

  if (wantChrome) {
    try {
      framePx = parseFloat(getComputedStyle(doc).getPropertyValue("--frame")) || 10;
    } catch (e) { framePx = 10; }
    var spanW = MIN_COL_W;
    var xCount = 0;
    var yCount = 0;
    var yBase = -1;
    var activeCol = -1;
    var px = -1;
    var py = -1;
    var dirty = true;
    var ticking = false;

    function buildX() {
      var want = Math.max(6, Math.ceil(window.innerWidth / MIN_COL_W));
      if (want === xCount) { return; }
      xCount = want;
      var html = "";
      for (var i = 0; i < want; i++) { html += "<span>" + colName(i) + "</span>"; }
      rulerX.innerHTML = html;
      spanW = rulerX.clientWidth / want || MIN_COL_W;
      activeCol = -1;
    }

    function buildY() {
      var want = Math.ceil(window.innerHeight / ROW_H) + 2;
      if (want !== yCount) {
        yCount = want;
        var html = "";
        for (var i = 0; i < want; i++) { html += "<span></span>"; }
        rulerY.innerHTML = html;
        yBase = -1;
      }
    }

    function paintY() {
      var base = Math.max(0, Math.floor(window.scrollY / ROW_H));
      if (base !== yBase) {
        yBase = base;
        var spans = rulerY.children;
        for (var i = 0; i < spans.length; i++) {
          spans[i].textContent = String(base + i + 1);
        }
      }
      var shift = -(window.scrollY - base * ROW_H);
      rulerY.style.transform = "translateY(" + shift + "px)";
    }

    function paint() {
      paintY();

      if (px >= 0) {
        var idx = Math.floor((px - framePx) / spanW);
        var max = xCount - 1;
        if (idx < 0) { idx = 0; }
        if (idx > max) { idx = max; }
        if (idx !== activeCol) {
          if (activeCol >= 0 && rulerX.children[activeCol]) {
            rulerX.children[activeCol].classList.remove("on");
          }
          activeCol = idx;
          if (rulerX.children[idx]) { rulerX.children[idx].classList.add("on"); }
        }
        if (chV && chH && chM) {
          chV.style.transform = "translateX(" + px + "px)";
          chH.style.transform = "translateY(" + py + "px)";
          chM.style.transform = "translate(" + px + "px," + py + "px)";
        }
        if (navRef) {
          var row = Math.floor((window.scrollY + py) / ROW_H) + 1;
          navRef.textContent = colName(idx) + row;
        }
      } else if (navRef) {
        var r0 = Math.floor(window.scrollY / ROW_H) + 1;
        navRef.textContent = "A" + r0;
      }
    }

    function requestPaint() {
      dirty = true;
      if (!ticking) {
        ticking = true;
        window.requestAnimationFrame(function () {
          ticking = false;
          if (dirty) { dirty = false; paint(); }
        });
      }
    }

    window.addEventListener("pointermove", function (e) {
      px = e.clientX;
      py = e.clientY;
      document.body.classList.add("has-pointer");
      requestPaint();
    }, { passive: true });

    window.addEventListener("scroll", requestPaint, { passive: true });
    window.addEventListener("resize", function () {
      try {
        framePx = parseFloat(getComputedStyle(doc).getPropertyValue("--frame")) || 10;
      } catch (e) { framePx = 10; }
      buildX();
      buildY();
      spanW = rulerX.clientWidth / xCount || MIN_COL_W;
      requestPaint();
    });

    buildX();
    buildY();
    requestPaint();
  }

  /* ---------------------------------------------------- reveal on scroll */
  var reveals = document.querySelectorAll(".reveal");
  if ("IntersectionObserver" in window && reveals.length) {
    var io = new IntersectionObserver(function (entries) {
      for (var i = 0; i < entries.length; i++) {
        if (entries[i].isIntersecting) {
          entries[i].target.classList.add("in");
          io.unobserve(entries[i].target);
        }
      }
    }, { threshold: 0.05, rootMargin: "0px 0px -6% 0px" });
    for (var r = 0; r < reveals.length; r++) { io.observe(reveals[r]); }
  } else {
    for (var q = 0; q < reveals.length; q++) { reveals[q].classList.add("in"); }
  }

  /* --------------------------------------------------- meters fill in view */
  var fills = document.querySelectorAll(".meter-fill");
  if ("IntersectionObserver" in window && fills.length && !reduceMotion) {
    var mio = new IntersectionObserver(function (entries) {
      for (var i = 0; i < entries.length; i++) {
        if (entries[i].isIntersecting) {
          entries[i].target.classList.add("in");
          mio.unobserve(entries[i].target);
        }
      }
    }, { threshold: 0.35 });
    for (var f = 0; f < fills.length; f++) { mio.observe(fills[f]); }
  } else {
    for (var m = 0; m < fills.length; m++) { fills[m].classList.add("in"); }
  }

  /* ------------------------------------------------------- stat count-up */
  /* The HTML already holds the final value; JS only ticks toward it,
     once, in view, with tabular numerals. Reduced motion: untouched. */
  var nums = document.querySelectorAll(".n[data-count]");
  if (!reduceMotion && "IntersectionObserver" in window && nums.length) {
    var nio = new IntersectionObserver(function (entries) {
      for (var i = 0; i < entries.length; i++) {
        var el = entries[i].target;
        if (!entries[i].isIntersecting) { continue; }
        nio.unobserve(el);
        countUp(el);
      }
    }, { threshold: 0.6 });
    for (var n = 0; n < nums.length; n++) { nio.observe(nums[n]); }
  }

  function countUp(el) {
    var final = parseFloat(el.getAttribute("data-count"));
    if (!isFinite(final)) { return; }
    var dec = parseInt(el.getAttribute("data-dec") || "0", 10) || 0;
    var exact = el.textContent;
    var t0 = null;
    var DUR = 700;
    function step(ts) {
      if (t0 === null) { t0 = ts; }
      var t = Math.min(1, (ts - t0) / DUR);
      var p = 1 - Math.pow(1 - t, 3);
      el.textContent = (final * p).toFixed(dec);
      if (t < 1) {
        window.requestAnimationFrame(step);
      } else {
        el.textContent = exact;
      }
    }
    window.requestAnimationFrame(step);
  }

  /* ------------------------------------------------------------ scrollspy */
  var links = document.querySelectorAll(".nav-links a");
  if ("IntersectionObserver" in window && links.length) {
    var byId = {};
    for (var l = 0; l < links.length; l++) {
      byId[links[l].getAttribute("href").slice(1)] = links[l];
    }
    var sio = new IntersectionObserver(function (entries) {
      for (var i = 0; i < entries.length; i++) {
        var a = byId[entries[i].target.id];
        if (!a) { continue; }
        if (entries[i].isIntersecting) {
          for (var k in byId) {
            if (byId[k].classList) { byId[k].classList.remove("active"); }
          }
          a.classList.add("active");
        }
      }
    }, { rootMargin: "-45% 0px -50% 0px" });
    var secs = document.querySelectorAll("main section[id]");
    for (var s = 0; s < secs.length; s++) { sio.observe(secs[s]); }
  }

  /* --------------------------------------------------------- copy buttons */
  var btns = document.querySelectorAll(".copy-btn");
  for (var b = 0; b < btns.length; b++) {
    btns[b].addEventListener("click", function () {
      var btn = this;
      var src = document.getElementById(btn.getAttribute("data-copy"));
      if (!src) { return; }
      var text = src.textContent;
      var done = function () {
        btn.classList.add("ok");
        btn.textContent = "COPIED";
        window.setTimeout(function () {
          btn.classList.remove("ok");
          btn.textContent = "COPY";
        }, 1400);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () { fallbackCopy(text, done); });
      } else {
        fallbackCopy(text, done);
      }
    });
  }

  function fallbackCopy(text, done) {
    var ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand("copy"); } catch (e) { /* copy is an enhancement */ }
    document.body.removeChild(ta);
    done();
  }
})();
