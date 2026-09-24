// Marqueet admin: drag to reorder leagues. Hand-written, no dependencies.
// Without this script the page still works: each league has an order box.
"use strict";

(function () {
  const list = document.getElementById("leagues");
  if (!list) return;
  document.documentElement.classList.add("js");

  function renumber() {
    list.querySelectorAll("li").forEach(function (li, i) {
      li.querySelector("input.order").value = String(i + 1);
    });
  }

  let dragging = null;

  list.addEventListener("pointerdown", function (e) {
    const grip = e.target.closest(".grip");
    if (!grip) return;
    dragging = grip.closest("li");
    dragging.classList.add("dragging");
    grip.setPointerCapture(e.pointerId);
    e.preventDefault();
  });

  list.addEventListener("pointermove", function (e) {
    if (!dragging) return;
    // Put the dragged row before the first row whose middle is below the pointer.
    const rows = Array.from(list.querySelectorAll("li")).filter(function (li) {
      return li !== dragging;
    });
    const next = rows.find(function (li) {
      const r = li.getBoundingClientRect();
      return e.clientY < r.top + r.height / 2;
    });
    list.insertBefore(dragging, next || null);
  });

  function drop() {
    if (!dragging) return;
    dragging.classList.remove("dragging");
    dragging = null;
    renumber();
  }
  list.addEventListener("pointerup", drop);
  list.addEventListener("pointercancel", drop);

  // Keyboard: Alt+Up / Alt+Down on a league checkbox moves the row.
  list.addEventListener("keydown", function (e) {
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    const li = e.target.closest("li");
    if (!li) return;
    if (e.key === "ArrowUp" && li.previousElementSibling) list.insertBefore(li, li.previousElementSibling);
    if (e.key === "ArrowDown" && li.nextElementSibling) list.insertBefore(li.nextElementSibling, li);
    e.target.focus();
    e.preventDefault();
    renumber();
  });
})();

// Widget slots: drag one slot onto another to swap their widgets. The
// selects stay the source of truth (and work without this script).
(function () {
  const slots = document.getElementById("slots");
  if (!slots) return;
  let from = null;

  function slotAt(x, y) {
    const el = document.elementFromPoint(x, y);
    return el && el.closest ? el.closest(".slot") : null;
  }

  slots.addEventListener("pointerdown", function (e) {
    if (e.target.closest("select")) return;
    from = e.target.closest(".slot");
    if (!from) return;
    from.classList.add("dragging");
    from.setPointerCapture(e.pointerId);
    e.preventDefault();
  });

  slots.addEventListener("pointermove", function (e) {
    if (!from) return;
    slots.querySelectorAll(".slot").forEach(function (s) {
      s.classList.toggle("drag-over", s === slotAt(e.clientX, e.clientY) && s !== from);
    });
  });

  function drop(e) {
    if (!from) return;
    const to = e.type === "pointerup" ? slotAt(e.clientX, e.clientY) : null;
    if (to && to !== from) {
      // Swap every picker (the widget and its options) with its twin.
      const a = from.querySelectorAll("select");
      const b = to.querySelectorAll("select");
      a.forEach(function (sa, i) {
        const sb = b[i];
        if (!sb) return;
        const v = sa.value;
        sa.value = sb.value;
        sb.value = v;
      });
    }
    slots.querySelectorAll(".slot").forEach(function (s) {
      s.classList.remove("drag-over", "dragging");
    });
    from = null;
  }
  slots.addEventListener("pointerup", drop);
  slots.addEventListener("pointercancel", drop);
})();
