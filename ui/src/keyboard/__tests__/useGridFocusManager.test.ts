import { describe, it, expect } from "vitest";
import { createGridFocusManager } from "../useGridFocusManager";

describe("createGridFocusManager", () => {
  it("initial state is (-1, -1)", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    expect(mgr.colIndex()).toBe(-1);
    expect(mgr.rowIndex()).toBe(-1);
  });

  it("moveDown from initial goes to (0, 0)", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.moveDown();
    expect(mgr.colIndex()).toBe(0);
    expect(mgr.rowIndex()).toBe(0);
  });

  it("moveDown increments row within column", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(0, 0);
    mgr.moveDown();
    expect(mgr.colIndex()).toBe(0);
    expect(mgr.rowIndex()).toBe(1);
    mgr.moveDown();
    expect(mgr.rowIndex()).toBe(2);
  });

  it("moveDown at bottom of column stays at bottom", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(0, 2);
    mgr.moveDown();
    expect(mgr.colIndex()).toBe(0);
    expect(mgr.rowIndex()).toBe(2);
  });

  it("moveUp decrements row", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(0, 2);
    mgr.moveUp();
    expect(mgr.colIndex()).toBe(0);
    expect(mgr.rowIndex()).toBe(1);
  });

  it("moveUp at top stays at 0", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(0, 0);
    mgr.moveUp();
    expect(mgr.colIndex()).toBe(0);
    expect(mgr.rowIndex()).toBe(0);
  });

  it("moveRight increments column, preserves row (clamped)", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(0, 2);
    mgr.moveRight();
    expect(mgr.colIndex()).toBe(1);
    // row 2 clamped to column 1's max (1)
    expect(mgr.rowIndex()).toBe(1);
  });

  it("moveLeft decrements column, preserves row (clamped)", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(2, 1);
    mgr.moveLeft();
    expect(mgr.colIndex()).toBe(1);
    expect(mgr.rowIndex()).toBe(1);
  });

  it("moveRight skips empty columns", () => {
    const mgr = createGridFocusManager(() => [3, 0, 2]);
    mgr.select(0, 1);
    mgr.moveRight();
    // Should skip column 1 (empty) and land on column 2
    expect(mgr.colIndex()).toBe(2);
    expect(mgr.rowIndex()).toBe(1);
  });

  it("moveLeft skips empty columns", () => {
    const mgr = createGridFocusManager(() => [3, 0, 2]);
    mgr.select(2, 1);
    mgr.moveLeft();
    // Should skip column 1 (empty) and land on column 0
    expect(mgr.colIndex()).toBe(0);
    expect(mgr.rowIndex()).toBe(1);
  });

  it("select sets explicit position", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(1, 1);
    expect(mgr.colIndex()).toBe(1);
    expect(mgr.rowIndex()).toBe(1);
  });

  it("reset returns to (-1, -1)", () => {
    const mgr = createGridFocusManager(() => [3, 2, 2]);
    mgr.select(1, 1);
    mgr.reset();
    expect(mgr.colIndex()).toBe(-1);
    expect(mgr.rowIndex()).toBe(-1);
  });

  it("empty columnCounts keeps (-1, -1)", () => {
    const mgr = createGridFocusManager(() => []);
    mgr.moveDown();
    expect(mgr.colIndex()).toBe(-1);
    expect(mgr.rowIndex()).toBe(-1);
    mgr.moveRight();
    expect(mgr.colIndex()).toBe(-1);
    expect(mgr.rowIndex()).toBe(-1);
  });
});
