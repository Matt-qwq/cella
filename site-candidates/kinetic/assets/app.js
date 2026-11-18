/* ============================================================
   Cella — “Kinetic Gradient” site candidate
   Enhancement only: every state below has a static fallback in
   the HTML. No JS, no IntersectionObserver, or reduced-motion
   all leave the final, correct page.
   ============================================================ */
(function () {
  "use strict";

  var docEl = document.documentElement;
  docEl.classList.add("js");

  var mq = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  function reduced() { return !!(mq && mq.matches); }
  var hasIO = "IntersectionObserver" in window;

  /* ---------- header depth on scroll ---------- */
  var head = document.querySelector(".site-head");
  if (head) {
    var onScroll = function () {
      head.classList.toggle("scrolled", window.scrollY > 12);
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
  }

  /* ---------- reveal on scroll ---------- */
  var revealEls = Array.prototype.slice.call(document.querySelectorAll("[data-reveal]"));
  if (!hasIO || reduced()) {
    revealEls.forEach(function (el) { el.classList.add("revealed"); });
  } else {
    var rio = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) {
          en.target.classList.add("revealed");
          rio.unobserve(en.target);
        }
      });
    }, { threshold: 0.12, rootMargin: "0px 0px -36px 0px" });
    revealEls.forEach(function (el) { rio.observe(el); });
  }

  /* ---------- stat counters (tabular numerals, CSS handles the rest) ---------- */
  var counters = Array.prototype.slice.call(document.querySelectorAll("[data-count]"));
  function writeNum(el, val) {
    var dec = parseInt(el.getAttribute("data-decimals") || "0", 10);
    el.textContent = val.toFixed(dec);
  }
  function runCounter(el) {
    if (el.getAttribute("data-done")) return;
    el.setAttribute("data-done", "1");
    var target = parseFloat(el.getAttribute("data-count"));
    if (!isFinite(target)) return;
    if (reduced()) { writeNum(el, target); return; }
    var dur = 1500, t0 = null;
    var step = function (t) {
      if (t0 === null) t0 = t;
      var p = Math.min(1, (t - t0) / dur);
      var eased = 1 - Math.pow(1 - p, 4);
      writeNum(el, target * eased);
      if (p < 1) { requestAnimationFrame(step); } else { writeNum(el, target); }
    };
    requestAnimationFrame(step);
  }
  if (!hasIO || reduced()) {
    counters.forEach(runCounter);
  } else {
    var cio = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) { runCounter(en.target); cio.unobserve(en.target); }
      });
    }, { threshold: 0.5 });
    counters.forEach(function (el) { cio.observe(el); });
  }

  /* ---------- the recalculation wave ---------- */
  var grid = document.getElementById("ripple-grid");
  if (grid) {
    var states = {
      before: { "B4": "160", "D4": "1,440", "B6": "635", "D6": "5,715",
                 "E2": "18.9%", "E3": "22.8%", "E4": "25.2%", "E5": "33.1%" },
      after:  { "B4": "310", "D4": "2,790", "B6": "785", "D6": "7,065",
                 "E2": "15.3%", "E3": "18.5%", "E4": "39.5%", "E5": "26.7%" }
    };
    var fxAddr = document.getElementById("fx-addr");
    var fxText = document.getElementById("fx-text");
    var recChip = document.getElementById("rec-chip");
    var win = document.getElementById("demo-win");

    function cell(id) { return grid.querySelector('[data-c="' + id + '"]'); }
    function setVals(state) {
      Object.keys(state).forEach(function (id) {
        var c = cell(id);
        if (c) c.textContent = state[id];
      });
    }
    function tick(id) {
      var c = cell(id);
      if (!c) return;
      c.classList.remove("tick");
      void c.offsetWidth; /* restart the animation */
      c.classList.add("tick");
    }
    function update(id, addr, formula) {
      var c = cell(id);
      if (c) c.textContent = states.after[id];
      tick(id);
      if (addr && formula) {
        if (fxAddr) fxAddr.textContent = addr;
        if (fxText) fxText.textContent = formula;
      }
    }
    function setChip(on, txt) {
      if (!recChip) return;
      recChip.textContent = txt;
      recChip.classList.toggle("on", on);
    }

    if (reduced() || !hasIO) {
      /* static: the HTML already carries the recalculated result */
      if (fxAddr && fxText) { fxAddr.textContent = "D4"; fxText.textContent = "=B4*C4"; }
      setChip(true, "recalculated");
    } else {
      var running = false;
      var timer = null;
      var timerResolve = null;
      var sleep = function (ms) {
        return new Promise(function (res) {
          timerResolve = res;
          timer = setTimeout(function () { timerResolve = null; res(); }, ms);
        });
      };
      var stop = function () {
        running = false;
        if (timer) { clearTimeout(timer); timer = null; }
        var r = timerResolve;
        timerResolve = null;
        if (r) r();
      };

      var playLoop = async function () {
        while (running) {
          /* film cut: dim, reset to the before-state */
          if (win) win.classList.add("resetting");
          await sleep(340);
          if (!running) break;
          setVals(states.before);
          if (fxAddr) fxAddr.textContent = "B4";
          if (fxText) fxText.textContent = "160";
          setChip(false, "idle");
          if (win) win.classList.remove("resetting");

          await sleep(980);
          if (!running) break;

          /* a value arrives in B4 — an edit, or a command */
          var b4 = cell("B4");
          if (b4) b4.classList.add("editing");
          if (fxText) fxText.textContent = "310";
          await sleep(680);
          if (!running) { if (b4) b4.classList.remove("editing"); break; }
          if (b4) b4.classList.remove("editing");
          update("B4", "B4", "310");

          /* dependents, in dependency order */
          await sleep(330);
          if (!running) break;
          update("D4", "D4", "=B4*C4");
          await sleep(250);
          if (!running) break;
          update("B6", "B6", "=SUM(B2:B5)");
          await sleep(250);
          if (!running) break;
          update("D6", "D6", "=SUM(D2:D5)");
          await sleep(240);
          if (!running) break;
          if (fxAddr) fxAddr.textContent = "E4";
          if (fxText) fxText.textContent = "=D4/D$6";
          update("E2");
          await sleep(100);
          if (!running) break;
          update("E3");
          await sleep(100);
          if (!running) break;
          update("E4");
          await sleep(100);
          if (!running) break;
          update("E5");

          setChip(true, "recalculated · 8 cells");
          await sleep(2700);
        }
        /* if the loop is cut mid-play, leave the finished state */
        if (!running) {
          setVals(states.after);
          setChip(true, "recalculated");
          if (fxAddr) fxAddr.textContent = "D4";
          if (fxText) fxText.textContent = "=B4*C4";
        }
      };

      var vio = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) {
          if (en.isIntersecting && !running) {
            running = true;
            playLoop();
          } else if (!en.isIntersecting && running) {
            stop();
          }
        });
      }, { threshold: 0.25 });
      vio.observe(win || grid);
    }
  }

  /* ---------- the chart draws itself ---------- */
  var chart = document.getElementById("revenue-chart");
  if (chart && !reduced() && hasIO) {
    var line = chart.querySelector(".chart-line");
    var len = 0;
    try { len = line.getTotalLength(); } catch (e) { len = 0; }
    if (len > 0) {
      line.style.strokeDasharray = len;
      line.style.strokeDashoffset = len;
      chart.classList.add("armed");
      var dio = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) {
          if (!en.isIntersecting) return;
          chart.classList.add("done");
          requestAnimationFrame(function () {
            requestAnimationFrame(function () {
              line.style.transition = "stroke-dashoffset 1.6s cubic-bezier(0.65, 0.05, 0.35, 0.95) 0.15s";
              line.style.strokeDashoffset = "0";
            });
          });
          dio.disconnect();
        });
      }, { threshold: 0.4 });
      dio.observe(chart);
    }
  }

  /* ---------- terminal session ---------- */
  var term = document.getElementById("term");
  if (term) {
    var lines = Array.prototype.slice.call(term.querySelectorAll(".t-line"));
    if (reduced() || !hasIO) {
      lines.forEach(function (l) { l.classList.add("shown"); });
      term.classList.add("done");
    } else {
      var lio = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) {
          if (!en.isIntersecting) return;
          lines.forEach(function (l, i) {
            setTimeout(function () { l.classList.add("shown"); }, 150 + i * 420);
          });
          setTimeout(function () { term.classList.add("done"); }, 150 + lines.length * 420 + 500);
          lio.disconnect();
        });
      }, { threshold: 0.4 });
      lio.observe(term);
    }
  }
})();
