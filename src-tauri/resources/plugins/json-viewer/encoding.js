// The encoding menu the text, JSON, Markdown and CSV viewers share (the same file is in each
// plugin's folder: a plugin is self-contained). Folio finds the encoding of a file itself; this
// shows which one it used and lets a person choose another. Plain script, no dependencies.
(function (root) {
  "use strict";

  /** What `read` and `readRange` accept, with the name a person sees. "" is Folio's own choice. */
  const CHOICES = [
    ["utf-8", "UTF-8"],
    ["utf-16le", "UTF-16 LE"],
    ["utf-16be", "UTF-16 BE"],
    ["windows-1252", "Windows-1252"],
  ];
  const NAMES = Object.fromEntries(CHOICES);

  /** The name of the encoding a read used, with "BOM" when the file starts with a mark. */
  function name(r) {
    const base = NAMES[r.encoding] || String(r.encoding || "");
    return r.bom ? `${base} (BOM)` : base;
  }

  /** Puts the choices in a `<select>`; the first, "Auto", leaves the choice to Folio. */
  function fill(select) {
    const auto = document.createElement("option");
    auto.value = "";
    auto.textContent = "Auto";
    select.append(auto);
    for (const [value, text] of CHOICES) {
      const o = document.createElement("option");
      o.value = value;
      o.textContent = text;
      select.append(o);
    }
    select.title = "Encoding of the text";
    select.setAttribute("aria-label", "Encoding");
  }

  /** Back to Auto, for a new file: no name of the last file's encoding is left on the menu. */
  function reset(select) {
    select.options[0].textContent = "Auto";
    select.title = "Encoding of the text";
    select.value = "";
  }

  /** Says which encoding a read used: on "Auto", and in the tooltip. */
  function show(select, r, chosen) {
    const label = name(r);
    select.options[0].textContent = chosen ? "Auto" : `Auto: ${label}`;
    select.title = `Encoding of the text: ${label}`;
    select.value = chosen || "";
  }

  const api = { CHOICES, name, fill, show, reset };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.Enc = api;
})(typeof self !== "undefined" ? self : this);
