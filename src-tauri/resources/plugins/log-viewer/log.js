"use strict";
// What the log viewer needs to know about lines of a log, and how it cuts pieces of a file into
// whole lines. Nothing here touches the page, so it is tested on its own.
(function (root) {
  const LEVEL_WORDS = {
    fatal: "error",
    critical: "error",
    severe: "error",
    error: "error",
    err: "error",
    warn: "warn",
    warning: "warn",
    notice: "info",
    info: "info",
    debug: "debug",
    verbose: "debug",
    trace: "trace",
  };
  // A level word set off by brackets, spaces or an equals sign, near the start of the line:
  // `ERROR`, `[warn]`, `level=info`, `| DEBUG |`.
  const LEVEL =
    /(?:^|[\s[(=:|])(fatal|critical|severe|error|err|warn(?:ing)?|notice|info|debug|trace|verbose)(?=$|[\s\])=:|,])/i;
  const HORIZON = 100;
  const STAMPS = [
    /^\[?\d{4}-\d\d-\d\d[T ]\d\d:\d\d:\d\d(?:[.,]\d+)?(?:Z|[+-]\d\d:?\d\d)?\]?/,
    /^\[?\d{4}\/\d\d\/\d\d \d\d:\d\d:\d\d(?:[.,]\d+)?\]?/,
    /^\[?\d\d:\d\d:\d\d(?:[.,]\d+)?\]?/,
    /^[A-Z][a-z]{2} +\d{1,2} \d\d:\d\d:\d\d/,
  ];
  const CONTINUATION = /^(?:\s+\S|Caused by:|\.\.\. \d+ more|Traceback \(most recent call last\))/;

  /** `error`, `warn`, `info`, `debug`, `trace`, or "" when the line names no level. */
  function levelOf(line) {
    const m = LEVEL.exec(line.slice(0, HORIZON));
    return m ? LEVEL_WORDS[m[1].toLowerCase()] : "";
  }

  /** How many characters at the start of the line are a timestamp (0 if none). */
  function stampLength(line) {
    for (const re of STAMPS) {
      const m = re.exec(line);
      if (m) return m[0].length;
    }
    return 0;
  }

  /** Whether the line carries on the one before it: a stack frame, an indented detail. */
  const continues = (line) => CONTINUATION.test(line);

  /**
   * The level of each line: a line that names none takes the level of the line it continues (a
   * stack trace under its error), and otherwise has none. `start` is the level to carry in.
   */
  function levels(lines, start = "") {
    const out = new Array(lines.length);
    let current = start;
    for (let i = 0; i < lines.length; i++) {
      const own = levelOf(lines[i]);
      if (own) current = own;
      else if (!continues(lines[i])) current = "";
      out[i] = current;
    }
    return out;
  }

  const utf8 = (s) => new TextEncoder().encode(s).length;

  /**
   * Cuts a piece of a file, read from byte `start`, down to whole lines, and says where the
   * whole lines begin and end in the file. A piece that doesn't begin at a line start loses its
   * first, partial line; one that doesn't end at the end of the file loses its last. A piece
   * with no line break in it is kept as it is, so a huge single line can't stall the reading.
   */
  function wholeLines(text, start, atLineStart, atEof) {
    let from = 0;
    let to = text.length;
    if (!atLineStart) {
      const nl = text.indexOf("\n");
      if (nl !== -1) from = nl + 1;
    }
    if (!atEof) {
      const nl = text.lastIndexOf("\n");
      if (nl !== -1 && nl + 1 > from) to = nl + 1;
    }
    const kept = text.slice(from, to);
    const first = start + utf8(text.slice(0, from));
    return { text: kept, start: first, end: first + utf8(kept) };
  }

  /**
   * Where in `text` the byte `bytes` falls, as a character index: the characters before it take
   * exactly that many bytes. -1 if it falls inside a character, or beyond the text.
   */
  function splitAt(text, bytes) {
    let used = 0;
    let i = 0;
    while (used < bytes && i < text.length) {
      const cp = text.codePointAt(i);
      used += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
      i += cp > 0xffff ? 2 : 1;
    }
    return used === bytes ? i : -1;
  }

  /** The lines of a piece's text, without the line breaks (and without an empty last line). */
  function linesOf(text) {
    if (!text) return [];
    const lines = text.split(/\r\n|\n|\r/);
    if (lines[lines.length - 1] === "") lines.pop();
    return lines;
  }

  root.Log = { levelOf, stampLength, continues, levels, wholeLines, linesOf, splitAt };
})(typeof globalThis !== "undefined" ? globalThis : window);
