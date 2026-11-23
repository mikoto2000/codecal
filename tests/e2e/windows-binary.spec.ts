import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import path from "node:path";

test("Windows release executable uses the GUI subsystem", () => {
  test.skip(process.platform !== "win32", "Windows executable check");
  const exe = path.resolve(
    process.env.CODECAL_E2E_EXE ?? "src-tauri/target/release/codecal.exe",
  );
  const binary = readFileSync(exe);
  expect(binary.toString("ascii", 0, 2)).toBe("MZ");
  const peOffset = binary.readUInt32LE(0x3c);
  expect(binary.toString("ascii", peOffset, peOffset + 4)).toBe("PE\0\0");
  // Subsystem is 68 bytes into the optional header for both PE32 and PE32+.
  // IMAGE_SUBSYSTEM_WINDOWS_GUI (2) prevents a console opening on launch.
  expect(binary.readUInt16LE(peOffset + 24 + 68)).toBe(2);
});
