import { act, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useMemo, useState } from "react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { MenuItem } from "./MenuItem";
import { Table } from "./Table";
import type { TableColumn, TableSort } from "./Table";
import { arrangeColumns, moveColumn, planDrop, readLayout, writeLayout } from "./tableLayout";
import { useListSelection } from "./useListSelection";

interface Movie {
  id: number;
  title: string;
  year: number;
}

const moviesOf = (count: number): Movie[] =>
  Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    title: `Movie ${i + 1}`,
    year: 1950 + (i % 70),
  }));

const columns: TableColumn<Movie>[] = [
  {
    id: "stage",
    label: "Stage",
    header: <span aria-hidden="true" />,
    width: 34,
    fixed: true,
    resizable: false,
    cell: () => "✓",
  },
  { id: "title", label: "Title", width: 200, grow: true, sortable: true, cell: (m) => m.title },
  { id: "year", label: "Year", width: 64, align: "end", sortable: true, cell: (m) => m.year },
  { id: "size", label: "Size", width: 90, align: "end", cell: (m) => `${m.id} GB` },
];

function Harness({
  rows,
  onOpen,
  storageKey,
  withMenu,
  withHeaderMenu,
  onSortChange,
}: {
  rows: Movie[];
  onOpen?: (id: number) => void;
  storageKey?: string;
  withMenu?: boolean;
  withHeaderMenu?: boolean;
  onSortChange?: (sort: TableSort) => void;
}) {
  const keys = useMemo(() => rows.map((m) => m.id), [rows]);
  const selection = useListSelection({ keys, onOpen });
  const [sort, setSort] = useState<TableSort | null>(null);
  return (
    <>
      <Table
        aria-label="Movies"
        rows={rows}
        rowKey={(m) => m.id}
        columns={columns}
        selection={selection}
        storageKey={storageKey}
        headerMenu={withHeaderMenu === true ? <MenuItem>Columns</MenuItem> : undefined}
        sort={sort}
        onSortChange={(next) => {
          setSort(next);
          onSortChange?.(next);
        }}
        menu={
          withMenu === true ? (
            <>
              <MenuItem>Rename {selection.count} titles</MenuItem>
            </>
          ) : undefined
        }
      />
      <output data-testid="count">{selection.count}</output>
      <output data-testid="current">{selection.current}</output>
    </>
  );
}

const rowOf = (title: string) => screen.getByText(title).closest('[role="row"]') as HTMLElement;
const count = () => screen.getByTestId("count").textContent;
const selectedTitles = () =>
  screen
    .getAllByRole("row", { hidden: true })
    .filter((row) => row.getAttribute("aria-selected") === "true")
    .map((row) => within(row).getAllByRole("gridcell", { hidden: true })[1]?.textContent);

// jsdom lays nothing out; give every element a window-sized box so the virtualiser has room.
const sizes = { clientHeight: 680, clientWidth: 1000 };
const originals = new Map<string, PropertyDescriptor | undefined>();
beforeAll(() => {
  for (const [name, value] of Object.entries(sizes)) {
    originals.set(name, Object.getOwnPropertyDescriptor(Element.prototype, name));
    Object.defineProperty(Element.prototype, name, { configurable: true, get: () => value });
  }
});
afterAll(() => {
  for (const [name, descriptor] of originals) {
    if (descriptor !== undefined) Object.defineProperty(Element.prototype, name, descriptor);
  }
});
beforeEach(() => {
  window.localStorage.clear();
});

describe("useListSelection by pointer", () => {
  it("selects one on click, adds and removes with Ctrl, and selects a range with Shift", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(10)} />);
    await user.click(rowOf("Movie 2"));
    expect(count()).toBe("1");
    await user.keyboard("{Shift>}");
    await user.click(rowOf("Movie 5"));
    await user.keyboard("{/Shift}");
    expect(count()).toBe("4");
    expect(selectedTitles()).toEqual(["Movie 2", "Movie 3", "Movie 4", "Movie 5"]);
    await user.keyboard("{Control>}");
    await user.click(rowOf("Movie 3"));
    await user.click(rowOf("Movie 8"));
    await user.keyboard("{/Control}");
    expect(selectedTitles()).toEqual(["Movie 2", "Movie 4", "Movie 5", "Movie 8"]);
    // Ctrl+Shift adds the range from the anchor (Movie 8) to the clicked one.
    await user.keyboard("{Control>}{Shift>}");
    await user.click(rowOf("Movie 10"));
    await user.keyboard("{/Shift}{/Control}");
    expect(count()).toBe("6");
    await user.click(rowOf("Movie 1"));
    expect(selectedTitles()).toEqual(["Movie 1"]);
  });

  it("opens on double-click", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(<Harness rows={moviesOf(5)} onOpen={onOpen} />);
    await user.dblClick(rowOf("Movie 3"));
    expect(onOpen).toHaveBeenCalledWith(3);
  });
});

describe("useListSelection by keyboard", () => {
  it("moves with the arrows, extends with Shift, moves alone with Ctrl and toggles with Space", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(10)} />);
    screen.getByRole("grid", { name: "Movies" }).focus();
    await user.keyboard("{ArrowDown}");
    expect(selectedTitles()).toEqual(["Movie 1"]);
    await user.keyboard("{ArrowDown}");
    expect(selectedTitles()).toEqual(["Movie 2"]);
    await user.keyboard("{Shift>}{ArrowDown}{ArrowDown}{/Shift}");
    expect(selectedTitles()).toEqual(["Movie 2", "Movie 3", "Movie 4"]);
    await user.keyboard("{Shift>}{ArrowUp}{/Shift}");
    expect(selectedTitles()).toEqual(["Movie 2", "Movie 3"]);
    await user.keyboard("{Control>}{ArrowDown}{ArrowDown}{/Control}");
    expect(count()).toBe("2");
    expect(rowOf("Movie 5")).toHaveAttribute("data-current", "true");
    await user.keyboard(" ");
    expect(selectedTitles()).toEqual(["Movie 2", "Movie 3", "Movie 5"]);
    await user.keyboard("{End}");
    expect(selectedTitles()).toEqual(["Movie 10"]);
    await user.keyboard("{Home}");
    expect(selectedTitles()).toEqual(["Movie 1"]);
  });

  it("selects everything with Ctrl+A, clears with Esc, and lets Esc through when nothing is selected", async () => {
    const user = userEvent.setup();
    const outer = vi.fn();
    render(
      // eslint-disable-next-line jsx-a11y/no-static-element-interactions
      <div onKeyDown={(event) => outer(event.key)}>
        <Harness rows={moviesOf(30)} />
      </div>,
    );
    screen.getByRole("grid").focus();
    await user.keyboard("{Control>}a{/Control}");
    expect(count()).toBe("30");
    await user.keyboard("{Escape}");
    expect(count()).toBe("0");
    expect(outer).not.toHaveBeenCalledWith("Escape");
    await user.keyboard("{Escape}");
    expect(outer).toHaveBeenCalledWith("Escape");
  });

  it("opens the current item with Enter", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(<Harness rows={moviesOf(5)} onOpen={onOpen} />);
    screen.getByRole("grid").focus();
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(onOpen).toHaveBeenCalledWith(2);
  });

  it("steps by rows in a grid of columns and drops items that leave the list", () => {
    const { result, rerender } = renderHook(({ keys }) => useListSelection({ keys, columns: 4 }), {
      initialProps: { keys: [1, 2, 3, 4, 5, 6, 7, 8, 9] },
    });
    const key = (k: string) => new KeyboardEvent("keydown", { key: k });
    act(() => void result.current.onKeyDown(key("ArrowRight")));
    act(() => void result.current.onKeyDown(key("ArrowDown")));
    expect(result.current.current).toBe(5);
    act(() => void result.current.onKeyDown(key("ArrowRight")));
    expect(result.current.current).toBe(6);
    act(() => result.current.select([5, 6, 7]));
    rerender({ keys: [1, 2, 3, 5, 7] });
    expect([...result.current.selected].sort()).toEqual([5, 7]);
    rerender({ keys: [1, 2, 3, 5, 6, 7] });
    expect([...result.current.selected].sort()).toEqual([5, 7]);
  });
});

describe("Table", () => {
  it("renders only the visible window of 20,000 rows", () => {
    render(<Harness rows={moviesOf(20_000)} />);
    const grid = screen.getByRole("grid", { name: "Movies" });
    expect(grid).toHaveAttribute("aria-rowcount", "20001");
    const bodyRows = screen.getAllByRole("row").length - 1;
    expect(bodyRows).toBeGreaterThan(10);
    expect(bodyRows).toBeLessThan(60);
    // 20,000 rows of 34px plus 16px of room under the last one.
    const body = screen.getAllByRole("rowgroup")[1];
    expect(body?.style.height).toBe(`${20_000 * 34 + 16}px`);
  });

  it("draws a 30px muted header and 34px rows, the selected tint and the current gold edge", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(5)} />);
    const header = screen.getAllByRole("row")[0];
    expect(header?.className).toContain("h-(--h-thead)");
    const title = screen.getByRole("columnheader", { name: "Title" });
    expect(title.className).toContain("font-(weight:--fw-600)");
    expect(title.className).toContain("text-text-muted");
    expect(rowOf("Movie 1").style.height).toBe("34px");
    expect(rowOf("Movie 1").className).toContain("border-line-row");
    await user.click(rowOf("Movie 2"));
    expect(rowOf("Movie 2").className).toContain("bg-selected-row");
    expect(rowOf("Movie 2").className).toContain("shadow-[inset_2px_0_0_var(--gold)]");
    expect(rowOf("Movie 1").className).not.toContain("bg-selected-row");
    expect(rowOf("Movie 1").className).toContain("cursor-pointer");
  });

  it("sorts from a header: ascending first, then descending, with an arrow", async () => {
    const user = userEvent.setup();
    const onSortChange = vi.fn();
    render(<Harness rows={moviesOf(5)} onSortChange={onSortChange} />);
    await user.click(screen.getByRole("button", { name: "Year" }));
    expect(onSortChange).toHaveBeenLastCalledWith({ column: "year", direction: "asc" });
    const year = screen.getByRole("columnheader", { name: "Year" });
    expect(year).toHaveAttribute("aria-sort", "ascending");
    expect(year.querySelector("svg")).not.toBeNull();
    await user.click(screen.getByRole("button", { name: "Year" }));
    expect(onSortChange).toHaveBeenLastCalledWith({ column: "year", direction: "desc" });
    expect(year).toHaveAttribute("aria-sort", "descending");
  });

  it("keeps a sortable column wide enough for its label and arrow, and reserves the arrow's place", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(5)} />);
    const year = screen.getByRole("columnheader", { name: "Year" });
    // 64px was asked for; "Year" with its padding and arrow needs more, so it never reads "Y…".
    expect(Number.parseInt(year.style.width, 10)).toBeGreaterThanOrEqual(81);
    expect(screen.getByRole("separator", { name: "Resize Year" })).toHaveAttribute(
      "aria-valuemin",
      "81",
    );
    // The arrow's slot is there before any sort, so sorting moves nothing.
    expect(year.querySelector("[data-sort-slot]")).not.toBeNull();
    expect(year.querySelector("svg")).toBeNull();
    // A column that does not sort keeps its asked width and has no slot.
    expect(screen.getByRole("columnheader", { name: "Size" }).style.width).toBe("90px");
    expect(screen.getByRole("columnheader", { name: "Size" })).not.toHaveAttribute("aria-sort");
    expect(year).toHaveAttribute("aria-sort", "none");
    await user.click(screen.getByRole("button", { name: "Year" }));
    expect(year.querySelector("svg")).not.toBeNull();
    // Only the sorted column shows an arrow.
    expect(screen.getByRole("columnheader", { name: "Title" }).querySelector("svg")).toBeNull();
    expect(screen.getByRole("columnheader", { name: "Title" })).toHaveAttribute(
      "aria-sort",
      "none",
    );
  });

  it("resizes a column by dragging its edge or with the arrows, and remembers the width", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<Harness rows={moviesOf(5)} storageKey="movies" />);
    const handle = screen.getByRole("separator", { name: "Resize Year" });
    expect(handle.className).toContain("cursor-col-resize");
    await user.pointer([
      { keys: "[MouseLeft>]", target: handle, coords: { clientX: 100 } },
      { target: handle, coords: { clientX: 160 } },
      { keys: "[/MouseLeft]", target: handle },
    ]);
    // Year asks for 64px, but its label and sort arrow need 81: it starts there, then grows by 60.
    expect(handle).toHaveAttribute("aria-valuenow", "141");
    handle.focus();
    await user.keyboard("{ArrowRight}");
    expect(handle).toHaveAttribute("aria-valuenow", "157");
    expect(readLayout("movies").widths).toEqual({ year: 157 });
    unmount();
    render(<Harness rows={moviesOf(5)} storageKey="movies" />);
    expect(screen.getByRole("columnheader", { name: "Year" }).style.width).toBe("157px");
  });

  it("reorders columns by dragging a header or with Alt+arrows, keeping fixed columns in place", async () => {
    const user = userEvent.setup();
    const headers = () => screen.getAllByRole("columnheader").map((h) => h.dataset.column);
    const { unmount } = render(<Harness rows={moviesOf(5)} storageKey="order" />);
    expect(headers()).toEqual(["stage", "title", "year", "size"]);
    // The size header is dragged left past the middle of Year (x 34 + 200 + 32 = 266).
    const size = screen.getByRole("columnheader", { name: "Size" });
    await user.pointer([
      { keys: "[MouseLeft>]", target: size, coords: { clientX: 340 } },
      { target: size, coords: { clientX: 250 } },
      { keys: "[/MouseLeft]", target: size, coords: { clientX: 250 } },
    ]);
    expect(headers()).toEqual(["stage", "title", "size", "year"]);
    screen.getByRole("button", { name: "Title" }).focus();
    await user.keyboard("{Alt>}{ArrowRight}{/Alt}");
    expect(headers()).toEqual(["stage", "size", "title", "year"]);
    // Nothing moves into the fixed stage column's place.
    screen.getByRole("separator", { name: "Resize Size" }).focus();
    await user.keyboard("{Alt>}{ArrowLeft}{/Alt}");
    expect(headers()).toEqual(["stage", "size", "title", "year"]);
    unmount();
    render(<Harness rows={moviesOf(5)} storageKey="order" />);
    expect(headers()).toEqual(["stage", "size", "title", "year"]);
  });

  it("moves the lifted header and the cells with the pointer without rendering a row", async () => {
    const user = userEvent.setup();
    let cellCalls = 0;
    const counted: TableColumn<Movie>[] = columns.map((c) =>
      c.id === "size" ? { ...c, cell: (m) => (cellCalls++, `${m.id} GB`) } : c,
    );
    function Counted() {
      const rows = useMemo(() => moviesOf(5), []);
      const keys = useMemo(() => rows.map((m) => m.id), [rows]);
      const selection = useListSelection({ keys });
      return (
        <Table
          aria-label="Movies"
          rows={rows}
          rowKey={(m) => m.id}
          columns={counted}
          selection={selection}
        />
      );
    }
    render(<Counted />);
    const grid = screen.getByRole("grid");
    const size = screen.getByRole("columnheader", { name: "Size" });
    const sizeCell = within(rowOf("Movie 1")).getAllByRole("gridcell")[3] as HTMLElement;
    const before = cellCalls;
    await user.pointer([
      { keys: "[MouseLeft>]", target: size, coords: { clientX: 340 } },
      { target: size, coords: { clientX: 330 } },
      { target: size, coords: { clientX: 250 } },
    ]);
    await act(async () => {
      await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    });
    // Lifted and following the pointer (90px left of where it was pressed), the neighbours slid.
    expect(grid).toHaveAttribute("data-reordering");
    expect(size).toHaveAttribute("data-lifted");
    expect(size.style.transform).toBe("translateX(-90px)");
    expect(sizeCell.style.transform).toBe("translateX(-90px)");
    expect(screen.getByRole("columnheader", { name: "Year" }).style.transform).toBe(
      "translateX(90px)",
    );
    expect(document.querySelector("[data-drop-marker]")).toHaveStyle({ display: "block" });
    // No row was rendered by any of it.
    expect(cellCalls).toBe(before);
    await user.pointer({ keys: "[/MouseLeft]", target: size, coords: { clientX: 250 } });
    expect(grid).not.toHaveAttribute("data-reordering");
    expect(size.style.transform).toBe("");
    expect(document.querySelector("[data-drop-marker]")).toHaveStyle({ display: "none" });
    expect(screen.getAllByRole("columnheader").map((h) => h.dataset.column)).toEqual([
      "stage",
      "title",
      "size",
      "year",
    ]);
  });

  it("puts a dragged header back on Esc and does not sort or clear the selection", async () => {
    const user = userEvent.setup();
    const onSortChange = vi.fn();
    render(<Harness rows={moviesOf(5)} storageKey="cancel" onSortChange={onSortChange} />);
    await user.click(rowOf("Movie 2"));
    const size = screen.getByRole("columnheader", { name: "Size" });
    await user.pointer([
      { keys: "[MouseLeft>]", target: size, coords: { clientX: 340 } },
      { target: size, coords: { clientX: 250 } },
    ]);
    expect(size).toHaveAttribute("data-lifted");
    await user.keyboard("{Escape}");
    expect(size).not.toHaveAttribute("data-lifted");
    expect(size.style.transform).toBe("");
    await user.pointer({ keys: "[/MouseLeft]", target: size, coords: { clientX: 250 } });
    expect(screen.getAllByRole("columnheader").map((h) => h.dataset.column)).toEqual([
      "stage",
      "title",
      "year",
      "size",
    ]);
    expect(readLayout("cancel").order).toEqual([]);
    expect(onSortChange).not.toHaveBeenCalled();
    expect(count()).toBe("1");
  });

  it("shows the new width on the header and the cells while the edge is dragged, before release", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(5)} storageKey="live" />);
    const handle = screen.getByRole("separator", { name: "Resize Size" });
    const header = screen.getByRole("columnheader", { name: "Size" });
    const cell = within(rowOf("Movie 1")).getAllByRole("gridcell")[3] as HTMLElement;
    await user.pointer([
      { keys: "[MouseLeft>]", target: handle, coords: { clientX: 100 } },
      { target: handle, coords: { clientX: 130 } },
    ]);
    await act(async () => {
      await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));
    });
    expect(header.style.width).toBe("120px");
    expect(cell.style.width).toBe("120px");
    // Nothing is stored until the release.
    expect(readLayout("live").widths).toEqual({});
    await user.pointer({ keys: "[/MouseLeft]", target: handle, coords: { clientX: 130 } });
    expect(readLayout("live").widths).toEqual({ size: 120 });
    expect(header.style.width).toBe("120px");
  });

  it("keeps the width when a resize is cancelled with Esc, and never goes under the minimum", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(5)} storageKey="resize-cancel" />);
    const handle = screen.getByRole("separator", { name: "Resize Size" });
    const header = screen.getByRole("columnheader", { name: "Size" });
    await user.pointer([
      { keys: "[MouseLeft>]", target: handle, coords: { clientX: 100 } },
      { target: handle, coords: { clientX: 130 } },
    ]);
    await user.keyboard("{Escape}");
    expect(header.style.width).toBe("90px");
    await user.pointer({ keys: "[/MouseLeft]", target: handle, coords: { clientX: 130 } });
    expect(readLayout("resize-cancel").widths).toEqual({});
    await user.pointer([
      { keys: "[MouseLeft>]", target: handle, coords: { clientX: 100 } },
      { target: handle, coords: { clientX: -500 } },
      { keys: "[/MouseLeft]", target: handle, coords: { clientX: -500 } },
    ]);
    expect(handle).toHaveAttribute("aria-valuenow", "40");
  });

  it("fits a column to its content when its edge is double-clicked", async () => {
    const user = userEvent.setup();
    const rect = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        // Only a cell laid out at its natural size reports a width.
        const natural = this.style.width === "max-content";
        const width = natural ? (this.dataset.col === "size" ? 133.2 : 50) : 0;
        return {
          width,
          height: 0,
          top: 0,
          left: 0,
          right: width,
          bottom: 0,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        };
      });
    try {
      render(<Harness rows={moviesOf(5)} storageKey="fit" />);
      await user.dblClick(screen.getByRole("separator", { name: "Resize Size" }));
      expect(screen.getByRole("columnheader", { name: "Size" }).style.width).toBe("134px");
      expect(readLayout("fit").widths).toEqual({ size: 134 });
      // The measuring left no trace on the cells.
      expect(within(rowOf("Movie 1")).getAllByRole("gridcell")[3]?.style.width).toBe("134px");
    } finally {
      rect.mockRestore();
    }
  });

  it("moves the right-clicked column with Move left and Move right in the header menu", async () => {
    const user = userEvent.setup();
    const headers = () => screen.getAllByRole("columnheader").map((h) => h.dataset.column);
    render(<Harness rows={moviesOf(5)} storageKey="menu" withHeaderMenu />);
    const rightClick = (name: string) =>
      user.pointer({
        keys: "[MouseRight]",
        target: screen.getByRole("columnheader", { name }),
      });
    await rightClick("Size");
    await user.click(screen.getByRole("menuitem", { name: "Move left" }));
    expect(headers()).toEqual(["stage", "title", "size", "year"]);
    expect(readLayout("menu").order).toEqual(["stage", "title", "size", "year"]);
    await rightClick("Title");
    await user.click(screen.getByRole("menuitem", { name: "Move right" }));
    expect(headers()).toEqual(["stage", "size", "title", "year"]);
    // The first movable column has nowhere to go left, and the fixed column has no move items.
    await rightClick("Size");
    expect(screen.getByRole("menuitem", { name: "Move left" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    await rightClick("Stage");
    expect(screen.queryByRole("menuitem", { name: "Move left" })).toBeNull();
  });

  it("selects an unselected row alone on right-click and keeps a selection it is part of", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(8)} withMenu />);
    await user.click(rowOf("Movie 2"));
    await user.keyboard("{Shift>}");
    await user.click(rowOf("Movie 5"));
    await user.keyboard("{/Shift}");
    await user.pointer({ keys: "[MouseRight]", target: screen.getByText("Movie 3") });
    expect(screen.getByRole("menuitem", { name: "Rename 4 titles" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.pointer({ keys: "[MouseRight]", target: screen.getByText("Movie 7") });
    expect(selectedTitles()).toEqual(["Movie 7"]);
    expect(screen.getByRole("menuitem", { name: "Rename 1 titles" })).toBeInTheDocument();
  });

  it("opens the menu of the current row with Shift+F10", async () => {
    const user = userEvent.setup();
    render(<Harness rows={moviesOf(8)} withMenu />);
    screen.getByRole("grid").focus();
    await user.keyboard("{ArrowDown}{ArrowDown}{Shift>}{F10}{/Shift}");
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(screen.getByTestId("current")).toHaveTextContent("2");
    expect(screen.getByRole("menuitem", { name: "Rename 1 titles" })).toBeInTheDocument();
  });
});

describe("table layout", () => {
  it("puts stored columns in the user's order, new ones after, fixed ones in place", () => {
    const cols = [
      { id: "check", width: 50, fixed: true },
      { id: "title", width: 200 },
      { id: "year", width: 64 },
      { id: "rating", width: 70 },
    ];
    const arranged = arrangeColumns(cols, {
      order: ["year", "gone", "title", "year"],
      widths: { title: 10, year: 90 },
    });
    expect(arranged.map((c) => c.id)).toEqual(["check", "year", "title", "rating"]);
    expect(arranged.map((c) => c.width)).toEqual([50, 90, 40, 70]);
  });

  it("never moves a fixed column or into its place", () => {
    const fixed = new Set(["check"]);
    expect(moveColumn(["check", "a", "b"], fixed, "b", 1)).toEqual(["check", "b", "a"]);
    expect(moveColumn(["check", "a", "b"], fixed, "a", 0)).toBeNull();
    expect(moveColumn(["check", "a", "b"], fixed, "check", 2)).toBeNull();
  });

  it("plans where a dragged column lands from its middle, keeping it between the movable columns", () => {
    const slots = [
      { id: "check", left: 0, width: 50, fixed: true },
      { id: "a", left: 50, width: 100, fixed: false },
      { id: "b", left: 150, width: 100, fixed: false },
      { id: "c", left: 250, width: 100, fixed: false },
    ];
    // Not moved: stays, and no one else slides.
    const rest = planDrop(slots, "b", 150);
    expect(rest?.order).toEqual(["check", "a", "b", "c"]);
    expect([...(rest?.shifts.values() ?? [])]).toEqual([0, 0, 0]);
    // Past the middle of c: b goes last, c slides left by b's width.
    const right = planDrop(slots, "b", 260);
    expect(right?.order).toEqual(["check", "a", "c", "b"]);
    expect(right?.shifts.get("c")).toBe(-100);
    expect(right?.dropLeft).toBe(250);
    // Dragged far left, over the fixed column: kept at the first movable place.
    const left = planDrop(slots, "c", -300);
    expect(left?.order).toEqual(["check", "c", "a", "b"]);
    expect(left?.dragLeft).toBe(50);
    expect(left?.shifts.get("a")).toBe(100);
    // A fixed column is not draggable.
    expect(planDrop(slots, "check", 100)).toBeNull();
  });

  it("reads a damaged entry as no layout", () => {
    window.localStorage.setItem("cinevault.table.broken", "{not json");
    expect(readLayout("broken")).toEqual({ order: [], widths: {} });
    writeLayout("ok", { order: ["a"], widths: { a: 100 } });
    expect(readLayout("ok")).toEqual({ order: ["a"], widths: { a: 100 } });
  });
});

// The table's grid gets focus from a click on a row, as a desktop list does.
it("takes focus when a row is pressed", () => {
  render(<Harness rows={moviesOf(3)} />);
  fireEvent.mouseDown(rowOf("Movie 1"), { shiftKey: true });
  expect(screen.getByRole("grid")).toHaveFocus();
});

describe("Table onVisibleRange", () => {
  it("reports the rows on screen so a windowed list can load their pages", () => {
    const onVisibleRange = vi.fn();
    function Windowed() {
      const rows = moviesOf(200);
      const keys = useMemo(() => rows.map((m) => m.id), [rows]);
      const selection = useListSelection({ keys });
      return (
        <Table
          aria-label="Movies"
          rows={rows}
          rowKey={(m) => m.id}
          columns={columns}
          selection={selection}
          onVisibleRange={onVisibleRange}
        />
      );
    }
    render(<Windowed />);
    const [range] = onVisibleRange.mock.calls.at(-1) as [{ start: number; end: number }];
    expect(range.start).toBe(0);
    // 680px of 34px rows plus the overscan: some, but nowhere near all 200.
    expect(range.end).toBeGreaterThan(15);
    expect(range.end).toBeLessThan(60);
  });
});
