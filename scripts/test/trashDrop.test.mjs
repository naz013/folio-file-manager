import assert from "node:assert/strict";
import { test } from "node:test";
import { inBin, trashable } from "../../src/lib/trashDrop.ts";

test("what is already in an operating system's bin is not sent there again", () => {
  for (const p of [
    "C:\\$Recycle.Bin\\S-1-5-21\\$R1A2B3.txt",
    "d:\\$RECYCLE.BIN\\S-1\\$Rx",
    "C:\\$Recycle.Bin",
    "/home/a/.local/share/Trash/files/old.txt",
    "/home/a/.Trash/old.txt",
    "/Users/a/.Trash/x",
    "/media/a/disk/.Trash-1000/files/x",
    "/Volumes/X/.Trashes/501/x",
    "/Volumes/X/.Trash/x",
  ]) {
    assert.ok(inBin(p), p);
  }
});

test("ordinary paths, even ones that look a little like a bin, are trashed", () => {
  for (const p of [
    "C:\\Users\\a\\Trash\\x.txt",
    "C:\\Users\\a\\Recycle.Bin notes.txt",
    "C:\\a\\$Recycle.Bin.bak\\x",
    "/home/a/Trash/x",
    "/home/a/trash.txt",
    "D:\\stuff\\.Trash\\x",
    "/home/a/proj/.trash-cache/x",
    "/home/a/notes/.Trash/x",
  ]) {
    assert.ok(!inBin(p), p);
  }
  assert.deepEqual(trashable(["C:\\a.txt", "C:\\$Recycle.Bin\\S-1\\$R1", "C:\\b"]), [
    "C:\\a.txt",
    "C:\\b",
  ]);
  assert.deepEqual(trashable([]), []);
});
