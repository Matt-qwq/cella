/* ============================================================
   CELLA — "Terminal Ledger" site candidate
   Vanilla JS. No dependencies, no network, works from file://.
   JS only enhances: boot typing, phosphor toggle, bar reveal,
   and the demo sheet. Everything is readable without it.
   ============================================================ */

(function () {
  "use strict";

  function $(sel, root) { return (root || document).querySelector(sel); }
  function $$(sel, root) {
    return Array.prototype.slice.call((root || document).querySelectorAll(sel));
  }
  var REDUCED = !!(window.matchMedia &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches);

  function safeGet(k) {
    try { return window.localStorage.getItem(k); } catch (e) { return null; }
  }
  function safeSet(k, v) {
    try { window.localStorage.setItem(k, v); } catch (e) {}
  }

  /* ============ 1. PHOSPHOR / AMBER ============ */
  var PHOS_KEY = "cella-site-phos";
  var rootEl = document.documentElement;
  var phosBtn = $("#phos-toggle");
  var phosFKey = $("#fkey-phos");
  var themeMeta = $('meta[name="theme-color"]');

  function applyPhos(mode, persist) {
    rootEl.setAttribute("data-phos", mode);
    if (persist) { safeSet(PHOS_KEY, mode); }
    var label = mode === "amber" ? "PHOS:AMBER" : "PHOS:GREEN";
    if (phosBtn) {
      phosBtn.textContent = label;
      phosBtn.setAttribute("aria-pressed", String(mode === "amber"));
    }
    if (phosFKey) {
      phosFKey.setAttribute("aria-pressed", String(mode === "amber"));
    }
    if (themeMeta) {
      themeMeta.setAttribute("content", mode === "amber" ? "#0b0703" : "#050b07");
    }
  }
  function togglePhos() {
    applyPhos(rootEl.getAttribute("data-phos") === "amber" ? "green" : "amber", true);
  }
  var savedPhos = safeGet(PHOS_KEY);
  if (savedPhos === "amber" || savedPhos === "green") { applyPhos(savedPhos, false); }
  if (phosBtn) { phosBtn.addEventListener("click", togglePhos); }
  if (phosFKey) { phosFKey.addEventListener("click", togglePhos); }

  /* ============ 2. BOOT SEQUENCE ============ */
  /* The markup ships complete (JS-off and reduced-motion both see the
     finished log); with motion allowed we retype it in ~1.5 s, skippable. */
  var boot = $("#boot");
  if (boot && !REDUCED) {
    var bootOriginal = boot.innerHTML;
    var bootLines = $$(".boot-line", boot).map(function (el) {
      return el.textContent.replace(/\n$/, "");
    });
    var bootSkipped = false, bootTimer = 0, bl = 0, bc = 0, cur = null;
    var bootSkipBtn = $("#boot-skip");

    function bootFinish() {
      if (bootSkipped) { return; }
      bootSkipped = true;
      window.clearTimeout(bootTimer);
      boot.innerHTML = bootOriginal;
      boot.classList.remove("typing");
      boot.removeEventListener("click", bootSkip);
      window.removeEventListener("keydown", bootKey);
      if (bootSkipBtn) {
        bootSkipBtn.removeEventListener("click", bootSkip);
        bootSkipBtn.hidden = true;
      }
    }
    function bootSkip() { bootFinish(); }
    function bootKey(e) {
      if (e.key === "Escape" || e.key === "Enter" || e.key === " ") { bootFinish(); }
    }
    function bootStep() {
      if (bootSkipped) { return; }
      if (bl >= bootLines.length) { bootFinish(); return; }
      var line = bootLines[bl];
      if (bc === 0) {
        cur = document.createElement("span");
        cur.className = "boot-line";
        boot.appendChild(cur);
      }
      if (bc < line.length) {
        var take = Math.min(3, line.length - bc);
        cur.textContent += line.slice(bc, bc + take);
        bc += take;
        bootTimer = window.setTimeout(bootStep, 6);
      } else {
        boot.appendChild(document.createTextNode("\n"));
        bl += 1; bc = 0;
        bootTimer = window.setTimeout(bootStep, 85);
      }
    }
    boot.addEventListener("click", bootSkip);
    window.addEventListener("keydown", bootKey);
    if (bootSkipBtn) {
      bootSkipBtn.hidden = false;
      bootSkipBtn.addEventListener("click", bootSkip);
    }
    boot.classList.add("typing");
    boot.textContent = "";
    bootStep();
  }

  /* ============ 3. BAR REVEAL ============ */
  var barFills = $$(".bar-fill");
  if (barFills.length && !REDUCED && "IntersectionObserver" in window) {
    barFills.forEach(function (el, i) {
      el.dataset.w = el.style.width;
      el.style.width = "0%";
      el.style.transitionDelay = (i % 3) * 90 + "ms";
    });
    var barIO = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (!en.isIntersecting) { return; }
        $$(".bar-fill", en.target).forEach(function (el) {
          el.style.width = el.dataset.w;
        });
        barIO.unobserve(en.target);
      });
    }, { threshold: 0.3 });
    $$(".bar-group").forEach(function (g) { barIO.observe(g); });
  }

  /* ============ 4. THE DEMO SHEET ============ */
  /* A tiny, bounded evaluator: arithmetic, cell refs, SUM/AVERAGE with
     ranges. No eval(), capped tokens/depth/steps, hostile input becomes
     an error value — the same rule the real engine is held to. */
  var sheet = $("#sheet");
  if (sheet) {
    var MAX_TOKENS = 80, MAX_DEPTH = 24, MAX_STEPS = 4000;
    var cells = {};
    var formulaCount = 0;

    $$("#sheet td[data-cell]").forEach(function (td) {
      var addr = td.getAttribute("data-cell");
      var f = td.getAttribute("data-f");
      cells[addr] = {
        td: td,
        raw: f ? f : td.textContent,
        formula: !!f,
        seen: false
      };
      if (f) { formulaCount += 1; }
    });

    function literalValue(c) {
      if (c.raw === "") { return { empty: true }; }
      if (/^-?(\d+\.?\d*|\.\d+)$/.test(c.raw)) { return { n: parseFloat(c.raw) }; }
      return { s: c.raw };
    }

    function expandRange(a, b) {
      var ca = a.charCodeAt(0), cb = b.charCodeAt(0);
      var ra = parseInt(a.slice(1), 10), rb = parseInt(b.slice(1), 10);
      var c0 = Math.min(ca, cb), c1 = Math.max(ca, cb);
      var r0 = Math.min(ra, rb), r1 = Math.max(ra, rb);
      var out = [];
      for (var c = c0; c <= c1; c++) {
        for (var r = r0; r <= r1; r++) {
          out.push(String.fromCharCode(c) + r);
        }
      }
      return out;
    }

    function lex(src) {
      var toks = [], i = 0, ch;
      while (i < src.length) {
        ch = src[i];
        if (ch === " " || ch === "\t") { i += 1; continue; }
        if ((ch >= "0" && ch <= "9") || ch === ".") {
          var m = /^\d*\.?\d+/.exec(src.slice(i));
          if (!m) { return { err: "#SYNTAX!" }; }
          toks.push({ t: "num", v: parseFloat(m[0]) });
          i += m[0].length;
          continue;
        }
        if (/[A-Za-z]/.test(ch)) {
          var mm = /^[A-Za-z]+[0-9]*/.exec(src.slice(i));
          var word = mm[0].toUpperCase();
          if (word === "SUM" || word === "AVERAGE") {
            toks.push({ t: "fn", v: word });
          } else if (/^[A-E][1-6]$/.test(word)) {
            toks.push({ t: "ref", v: word });
          } else {
            return { err: "#NAME?" };
          }
          i += mm[0].length;
          continue;
        }
        if ("+-*/(),:".indexOf(ch) >= 0) {
          toks.push({ t: ch });
          i += 1;
          continue;
        }
        return { err: "#SYNTAX!" };
      }
      if (toks.length > MAX_TOKENS) { return { err: "#LIMIT!" }; }
      return toks;
    }

    function mkNum(n) { return function () { return { n: n }; }; }
    function mkRef(addr) { return function (ctx) { return ctx.value(addr); }; }
    function mkNeg(v) {
      return function (ctx) {
        var a = v(ctx);
        if (a.e) { return a; }
        if (a.s !== undefined || a.empty) { return { e: "#VALUE!" }; }
        return { n: -a.n };
      };
    }
    function mkBin(l, r, op) {
      return function (ctx) {
        var a = l(ctx), b = r(ctx);
        if (a.e) { return a; }
        if (b.e) { return b; }
        var x = a.empty ? 0 : (a.s !== undefined ? NaN : a.n);
        var y = b.empty ? 0 : (b.s !== undefined ? NaN : b.n);
        if (isNaN(x) || isNaN(y)) { return { e: "#VALUE!" }; }
        if (op === "+") { return { n: x + y }; }
        if (op === "-") { return { n: x - y }; }
        if (op === "*") { return { n: x * y }; }
        if (y === 0) { return { e: "#DIV/0!" }; }
        return { n: x / y };
      };
    }
    function mkFn(name, args) {
      return function (ctx) {
        var sum = 0, count = 0, i, j, vals, v;
        for (i = 0; i < args.length; i++) {
          if (args[i].range) {
            vals = args[i].addrs;
            for (j = 0; j < vals.length; j++) {
              v = ctx.value(vals[j]);
              if (v.e) { return v; }
              if (v.n !== undefined) { sum += v.n; count += 1; }
            }
          } else {
            v = args[i].ev(ctx);
            if (v.e) { return v; }
            if (v.n !== undefined) { sum += v.n; count += 1; }
          }
        }
        if (name === "AVERAGE" && count === 0) { return { e: "#DIV/0!" }; }
        return { n: name === "AVERAGE" ? sum / count : sum };
      };
    }

    function parse(src) {
      var toks = lex(src);
      if (toks.err) { return toks; }
      var p = 0, depth = 0;
      function peek(off) { return toks[p + (off || 0)]; }
      function expect(t) {
        var k = toks[p];
        if (k && k.t === t) { p += 1; return true; }
        return false;
      }
      function expr() {
        if (++depth > MAX_DEPTH) { throw "#LIMIT!"; }
        var v = mul();
        while (peek() && (peek().t === "+" || peek().t === "-")) {
          var op = toks[p].t; p += 1;
          v = mkBin(v, mul(), op);
        }
        depth -= 1;
        return v;
      }
      function mul() {
        if (++depth > MAX_DEPTH) { throw "#LIMIT!"; }
        var v = unary();
        while (peek() && (peek().t === "*" || peek().t === "/")) {
          var op = toks[p].t; p += 1;
          v = mkBin(v, unary(), op);
        }
        depth -= 1;
        return v;
      }
      function unary() {
        if (++depth > MAX_DEPTH) { throw "#LIMIT!"; }
        var v;
        if (peek() && peek().t === "-") { p += 1; v = mkNeg(unary()); }
        else if (peek() && peek().t === "+") { p += 1; v = unary(); }
        else { v = atom(); }
        depth -= 1;
        return v;
      }
      function atom() {
        if (++depth > MAX_DEPTH) { throw "#LIMIT!"; }
        var k = peek(), v;
        if (!k) { throw "#SYNTAX!"; }
        if (k.t === "num") { p += 1; v = mkNum(k.v); }
        else if (k.t === "ref") {
          p += 1;
          if (peek() && peek().t === ":") { throw "#SYNTAX!"; }
          v = mkRef(k.v);
        }
        else if (k.t === "fn") {
          p += 1;
          if (!expect("(")) { throw "#SYNTAX!"; }
          var args = [];
          if (peek() && peek().t !== ")") {
            args.push(parseArg());
            while (peek() && peek().t === ",") { p += 1; args.push(parseArg()); }
          }
          if (!expect(")")) { throw "#SYNTAX!"; }
          v = mkFn(k.v, args);
        }
        else if (k.t === "(") {
          p += 1;
          v = expr();
          if (!expect(")")) { throw "#SYNTAX!"; }
        }
        else { throw "#SYNTAX!"; }
        depth -= 1;
        return v;
      }
      function parseArg() {
        var a = peek(), b = peek(2);
        if (a && a.t === "ref" && peek(1) && peek(1).t === ":" && b && b.t === "ref") {
          p += 3;
          return { range: true, addrs: expandRange(a.v, b.v) };
        }
        return { range: false, ev: expr() };
      }
      try {
        var rootNode = expr();
        if (p !== toks.length) { return { err: "#SYNTAX!" }; }
        return { ok: rootNode };
      } catch (e) {
        return { err: (typeof e === "string") ? e : "#SYNTAX!" };
      }
    }

    function makeCtx() {
      var cache = {};
      var steps = { n: 0 };
      return {
        value: function (addr) {
          var hit = cache[addr];
          if (hit) { return hit; }
          steps.n += 1;
          if (steps.n > MAX_STEPS) { return { e: "#LIMIT!" }; }
          var c = cells[addr];
          if (!c) { return { e: "#REF!" }; }
          if (!c.formula) {
            var lv = literalValue(c);
            cache[addr] = lv;
            return lv;
          }
          if (c.seen) { return { e: "#CIRC!" }; }
          c.seen = true;
          var v;
          try {
            var parsed = parse(c.raw.slice(1));
            if (parsed.err) { v = { e: parsed.err }; }
            else { v = parsed.ok(this); }
          } catch (e2) {
            v = { e: "#ERR!" };
          }
          c.seen = false;
          if (!v.e) { cache[addr] = v; }
          return v;
        }
      };
    }

    function fmt(v) {
      if (!v) { return ""; }
      if (v.e) { return v.e; }
      if (v.empty) { return ""; }
      if (v.s !== undefined) { return v.s; }
      if (typeof v.n !== "number" || !isFinite(v.n)) { return "#ERR!"; }
      return v.n.toFixed(2);
    }

    function refList(raw) {
      var out = [], m;
      var re = /([A-E])([1-6])/g;
      while ((m = re.exec(raw)) !== null) { out.push(m[1] + m[2]); }
      return out;
    }
    function depthOf(addr, guard) {
      guard = guard || {};
      if (guard[addr]) { return 0; }
      guard[addr] = true;
      var c = cells[addr];
      var d = 0;
      if (c && c.formula) {
        d = 1;
        var refs = refList(c.raw);
        for (var i = 0; i < refs.length; i++) {
          d = Math.max(d, 1 + depthOf(refs[i], guard));
        }
      }
      guard[addr] = false;
      return d;
    }

    var logEl = $("#sheet-log");
    function log(msg) {
      if (logEl) { logEl.textContent = msg; }
    }

    function recalcAll(label, quiet) {
      var t0 = (window.performance && performance.now) ? performance.now() : Date.now();
      var ctx = makeCtx();
      var changed = [];
      var errors = 0;
      Object.keys(cells).forEach(function (addr) {
        var c = cells[addr];
        var disp;
        if (c.formula) {
          var v = ctx.value(addr);
          disp = fmt(v);
          if (v && v.e) { errors += 1; }
          if (v && v.e) { c.td.classList.add("err-val"); }
          else { c.td.classList.remove("err-val"); }
        } else {
          disp = c.raw;
        }
        if (disp !== c.td.textContent) {
          changed.push({ td: c.td, disp: disp, d: depthOf(addr) });
        }
      });
      var t1 = (window.performance && performance.now) ? performance.now() : Date.now();
      var ms = (t1 - t0).toFixed(1);
      changed.sort(function (a, b) { return a.d - b.d; });
      changed.forEach(function (ch) {
        ch.td.textContent = ch.disp;
        if (!REDUCED && !quiet) {
          ch.td.classList.remove("pulse");
          void ch.td.offsetWidth;
          ch.td.style.animationDelay = Math.min(ch.d * 70, 900) + "ms";
          ch.td.classList.add("pulse");
        }
      });
      if (!quiet) {
        log(label + " · " + changed.length + " CELL" +
          (changed.length === 1 ? "" : "S") + " UPDATED · " + ms + " MS" +
          (errors ? " · " + errors + " ERROR" + (errors === 1 ? "" : "S") : ""));
      }
      return changed.length;
    }

    sheet.addEventListener("animationend", function (e) {
      var td = e.target;
      if (td && td.classList && td.classList.contains("pulse")) {
        td.classList.remove("pulse");
        td.style.animationDelay = "";
      }
    });

    /* --- selection + formula bar --- */
    var fx = $("#fx-input");
    var nameBox = $("#cell-name");
    var selected = "D6";

    function select(addr, focusInput) {
      var c = cells[addr];
      if (!c) { return; }
      $$("#sheet td.selected").forEach(function (td) {
        td.classList.remove("selected");
      });
      c.td.classList.add("selected");
      selected = addr;
      if (nameBox) { nameBox.textContent = addr; }
      if (fx) {
        fx.value = c.raw;
        fx.setAttribute("aria-label", "Formula bar, cell " + addr);
      }
      if (focusInput && fx) { fx.focus(); }
    }

    function commit(addr, raw) {
      var c = cells[addr];
      if (!c) { return; }
      raw = String(raw).slice(0, 120).replace(/[\r\n\t]+/g, " ").trim();
      if (raw.charAt(0) === "=" && raw.length > 1) {
        c.formula = true;
        c.raw = raw;
        c.td.setAttribute("data-f", raw);
      } else {
        c.formula = false;
        c.raw = raw;
        if (c.td.hasAttribute("data-f")) { c.td.removeAttribute("data-f"); }
      }
      recalcAll("SET " + addr + " · RECALC OK", false);
      select(addr, false);
    }

    $$("#sheet td[data-cell]").forEach(function (td) {
      td.addEventListener("click", function () {
        select(td.getAttribute("data-cell"), false);
      });
      td.addEventListener("keydown", function (e) {
        var addr = td.getAttribute("data-cell");
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          select(addr, true);
          return;
        }
        var col = addr.charCodeAt(0), row = parseInt(addr.slice(1), 10);
        var nc = col, nr = row;
        if (e.key === "ArrowUp") { nr -= 1; }
        else if (e.key === "ArrowDown") { nr += 1; }
        else if (e.key === "ArrowLeft") { nc -= 1; }
        else if (e.key === "ArrowRight") { nc += 1; }
        else { return; }
        e.preventDefault();
        if (nc < 65 || nc > 69 || nr < 1 || nr > 6) { return; }
        var nextAddr = String.fromCharCode(nc) + nr;
        var next = cells[nextAddr];
        if (next) {
          select(nextAddr, false);
          next.td.focus();
        }
      });
    });

    if (fx) {
      fx.addEventListener("keydown", function (e) {
        if (e.key === "Enter") {
          e.preventDefault();
          commit(selected, fx.value);
        } else if (e.key === "Escape") {
          fx.value = cells[selected] ? cells[selected].raw : "";
        }
      });
    }
    var commitBtn = $("#btn-commit");
    if (commitBtn) {
      commitBtn.addEventListener("click", function () {
        if (fx) { commit(selected, fx.value); }
      });
    }
    var recalcBtn = $("#btn-recalc");
    if (recalcBtn) {
      recalcBtn.addEventListener("click", function () {
        recalcAll("RECALC OK", false);
      });
    }

    /* --- init: verify statics, select D6 --- */
    recalcAll("", true);
    select("D6", false);
    log("READY · " + formulaCount + " FORMULA CELLS · EDIT A PRICE AND PRESS RECALC");

    /* --- function keys --- */
    var win = sheet.closest ? sheet.closest(".sheet-window") : null;
    function gotoSheet() {
      if (win) { win.scrollIntoView(REDUCED ? { block: "center" } : { behavior: "smooth", block: "center" }); }
    }
    var fkEdit = $("#fkey-edit");
    if (fkEdit) {
      fkEdit.addEventListener("click", function () {
        gotoSheet();
        if (fx) { fx.focus(); fx.select(); }
      });
    }
    ["#fkey-recalc", "#fkey-calc"].forEach(function (id) {
      var b = $(id);
      if (b) {
        b.addEventListener("click", function () {
          gotoSheet();
          recalcAll("RECALC OK", false);
        });
      }
    });
  }
})();
