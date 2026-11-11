// The page a plugin would ship to draw a thumbnail of a CSV file (prototype for issue #121).
//
// It runs in a sandboxed frame like any plugin page. Folio sends
//   { folio: 1, event: "thumbnail", id, name, path, size, width, height }
// the page reads the start of the file through the usual `read` call, draws it on a canvas and
// answers
//   { folio: 1, event: "thumbnail", id, png: ArrayBuffer }       (or { id, error })
// The host does not trust the answer: it checks the PNG before it is shown (see harness.html).

(() => {
  const TAG = 1;
  let nextCall = 1;
  const pending = new Map();

  const ask = (method, params) =>
    new Promise((resolve, reject) => {
      const id = nextCall++;
      pending.set(id, { resolve, reject });
      parent.postMessage({ folio: TAG, id, method, params }, "*");
    });

  addEventListener("message", (e) => {
    const m = e.data;
    if (!m || m.folio !== TAG) return;
    if (typeof m.id === "number" && pending.has(m.id) && m.event === undefined) {
      const p = pending.get(m.id);
      pending.delete(m.id);
      if (m.error) p.reject(new Error(m.error));
      else p.resolve(m.result);
    } else if (m.event === "thumbnail") {
      draw(m).then(
        (png) => parent.postMessage({ folio: TAG, event: "thumbnail", id: m.id, png }, "*", [png]),
        (err) =>
          parent.postMessage({ folio: TAG, event: "thumbnail", id: m.id, error: String(err) }, "*"),
      );
    }
  });

  /** The first rows and columns of a CSV, split the simple way: good enough for a thumbnail. */
  function rows(text, maxRows, maxCols) {
    const out = [];
    for (const line of text.split(/\r?\n/)) {
      if (!line) continue;
      const delim = line.includes("\t")
        ? "\t"
        : line.includes(";") && !line.includes(",")
          ? ";"
          : ",";
      out.push(line.split(delim).slice(0, maxCols));
      if (out.length >= maxRows) break;
    }
    return out;
  }

  async function draw({ path, width, height }) {
    // 4 KB is plenty for the first rows; a bigger read would only cost more.
    const { text } = await ask("read", { path });
    const grid = rows(text.slice(0, 4096), 9, 4);
    const canvas = new OffscreenCanvas(width, height);
    const g = canvas.getContext("2d");
    g.fillStyle = "#fff";
    g.fillRect(0, 0, width, height);
    const pad = 8;
    const rowH = (height - 2 * pad - 24) / 9;
    const cols = Math.max(1, ...grid.map((r) => r.length));
    const colW = (width - 2 * pad) / cols;
    g.font = `${Math.round(rowH * 0.62)}px sans-serif`;
    g.textBaseline = "middle";
    grid.forEach((r, y) => {
      if (y === 0) {
        g.fillStyle = "#12a594";
        g.fillRect(pad, pad + y * rowH, width - 2 * pad, rowH);
      }
      r.forEach((cell, x) => {
        g.fillStyle = y === 0 ? "#fff" : "#333";
        g.save();
        g.beginPath();
        g.rect(pad + x * colW, pad + y * rowH, colW - 2, rowH);
        g.clip();
        g.fillText(cell.trim(), pad + x * colW + 2, pad + y * rowH + rowH / 2);
        g.restore();
      });
      g.fillStyle = "#d8dee4";
      g.fillRect(pad, pad + (y + 1) * rowH - 1, width - 2 * pad, 1);
    });
    const blob = await canvas.convertToBlob({ type: "image/png" });
    return blob.arrayBuffer();
  }

  parent.postMessage({ folio: TAG, event: "ready" }, "*");
})();
