/* ————————————————————————————————————————————————————————————————————————
   CELLA · EDITORIAL PAPER — the enhancement layer.

   The page is complete without this file: the contents are plain anchors,
   the notes are in the flow, the listings are text. What JavaScript adds:
     1. the contents list and a corner chip track the chapter being read;
     2. a quiet "copy" button appears on code listings (only if the
        clipboard is actually available, and only on secure contexts).
   Nothing animates, nothing is required, nothing phones home.
   ———————————————————————————————————————————————————————————————————————— */
(function () {
  'use strict';

  var chapters = Array.prototype.slice.call(document.querySelectorAll('[data-chapter]'));
  var chip = document.querySelector('.chip');
  var tocItems = Array.prototype.slice.call(document.querySelectorAll('.toc li'));

  /* ——— 1. Where you are in the issue ——————————————————————————————————— */
  function currentChapter() {
    /* The chapter whose heading has crossed two-fifths of the viewport. */
    var line = window.innerHeight * 0.4;
    var cur = null;
    chapters.forEach(function (section) {
      if (section.getBoundingClientRect().top <= line) cur = section;
    });
    return cur;
  }

  function update() {
    var cur = currentChapter();
    if (!cur) return;

    tocItems.forEach(function (li) {
      var link = li.querySelector('a');
      var on = link && link.getAttribute('href') === '#' + cur.id;
      li.classList.toggle('current', on);
      if (link) {
        if (on) link.setAttribute('aria-current', 'true');
        else link.removeAttribute('aria-current');
      }
    });

    if (chip) {
      chip.textContent = 'Ch. ' + cur.getAttribute('data-ch-no') + ' \u00b7 ' + cur.getAttribute('data-ch-name');
    }
  }

  if (chapters.length) {
    var ticking = false;
    window.addEventListener('scroll', function () {
      if (!ticking) {
        ticking = true;
        window.requestAnimationFrame(function () {
          update();
          ticking = false;
        });
      }
    }, { passive: true });
    window.addEventListener('resize', update, { passive: true });
    update();
    if (chip) chip.hidden = false;   /* the chip exists only once it can be right */
  }

  /* ——— 2. Copy buttons on the listings ————————————————————————————————— */
  if (navigator.clipboard && navigator.clipboard.writeText) {
    Array.prototype.forEach.call(document.querySelectorAll('.code-fig'), function (fig) {
      var pre = fig.querySelector('pre');
      if (!pre) return;

      var btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'copybtn';
      btn.textContent = 'Copy';

      btn.addEventListener('click', function () {
        navigator.clipboard.writeText(pre.textContent).then(function () {
          btn.textContent = 'Copied';
          btn.classList.add('did');
          window.setTimeout(function () {
            btn.textContent = 'Copy';
            btn.classList.remove('did');
          }, 1600);
        }).catch(function () {
          btn.textContent = 'Select it manually';
        });
      });

      fig.appendChild(btn);
    });
  }
}());
