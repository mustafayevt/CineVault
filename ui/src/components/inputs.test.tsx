import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { Checkbox } from "./Checkbox";
import { Chip } from "./Chip";
import { Field } from "./Field";
import { SearchField } from "./SearchField";
import { SegmentedControl } from "./SegmentedControl";
import { Select } from "./Select";
import { SurfaceContext } from "./surface";
import { Switch } from "./Switch";

describe("Field", () => {
  it("is 30px on the card colour with the edge border, and names itself", () => {
    render(<Field label="Title" />);
    const input = screen.getByLabelText("Title");
    const box = input.closest("[data-field]");
    expect(box?.className).toContain("h-(--h-field)");
    expect(box?.className).toContain("bg-surface-card");
    expect(box?.className).toContain("border-line-edge");
    expect(box?.className).toContain("rounded-(--r-6)");
  });

  it("takes the well colour on a floating surface, by prop or by context", () => {
    render(
      <>
        <Field aria-label="by prop" floating />
        <SurfaceContext value="floating">
          <Field aria-label="by context" />
          <Field aria-label="opted out" floating={false} />
        </SurfaceContext>
      </>,
    );
    expect(screen.getByLabelText("by prop").closest("[data-field]")?.className).toContain(
      "bg-surface-well",
    );
    expect(screen.getByLabelText("by context").closest("[data-field]")?.className).toContain(
      "bg-surface-well",
    );
    expect(screen.getByLabelText("opted out").closest("[data-field]")?.className).toContain(
      "bg-surface-card",
    );
  });

  it("connects the error and the helper text, and accepts typing", async () => {
    render(<Field label="Template" description="Use tokens." error="Unknown token." />);
    const input = screen.getByLabelText("Template");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Unknown token.");
    await userEvent.type(input, "{{Title}");
    expect(input).toHaveValue("{Title}");
  });

  it("is inert when disabled", async () => {
    render(<Field aria-label="Path" disabled />);
    const input = screen.getByLabelText("Path");
    expect(input).toBeDisabled();
    await userEvent.type(input, "x");
    expect(input).toHaveValue("");
  });
});

describe("SearchField", () => {
  it("is 34px with a search role", () => {
    render(<SearchField aria-label="Search" value="" onChange={() => {}} />);
    const input = screen.getByRole("searchbox", { name: "Search" });
    expect(input.closest("[data-field]")?.className).toContain("h-(--h-search)");
  });

  it("clears with the button and with Esc only while it has text", async () => {
    const onClear = vi.fn();
    const { rerender } = render(
      <SearchField aria-label="Search" value="" onChange={() => {}} onClear={onClear} />,
    );
    expect(screen.queryByRole("button", { name: "Clear search" })).toBeNull();
    await userEvent.type(screen.getByRole("searchbox"), "{Escape}");
    expect(onClear).not.toHaveBeenCalled();

    rerender(<SearchField aria-label="Search" value="arr" onChange={() => {}} onClear={onClear} />);
    await userEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onClear).toHaveBeenCalledTimes(1);
    screen.getByRole("searchbox").focus();
    await userEvent.keyboard("{Escape}");
    expect(onClear).toHaveBeenCalledTimes(2);
  });
});

describe("SegmentedControl keyboard", () => {
  const options = [
    { value: "a", label: "Posters" },
    { value: "b", label: "Table" },
    { value: "c", label: "Lists", count: 3 },
  ] as const;

  function Harness() {
    const [value, setValue] = useState<"a" | "b" | "c">("a");
    return (
      <SegmentedControl aria-label="View" options={options} value={value} onChange={setValue} />
    );
  }

  it("has one tab stop and moves and chooses with the arrows, wrapping at both ends", async () => {
    render(<Harness />);
    const [a, b, c] = ["Posters", "Table", "Lists3"].map((name) =>
      screen.getByRole("button", { name }),
    );
    expect(a).toHaveAttribute("tabindex", "0");
    expect(b).toHaveAttribute("tabindex", "-1");

    await userEvent.tab();
    expect(a).toHaveFocus();
    await userEvent.keyboard("{ArrowRight}");
    expect(b).toHaveFocus();
    expect(b).toHaveAttribute("aria-pressed", "true");
    await userEvent.keyboard("{End}");
    expect(c).toHaveFocus();
    expect(c).toHaveAttribute("aria-pressed", "true");
    await userEvent.keyboard("{ArrowRight}");
    expect(a).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(c).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(a).toHaveAttribute("aria-pressed", "true");
  });

  it("draws the 24px segments with the active one on the active surface at 600", () => {
    render(<Harness />);
    const active = screen.getByRole("button", { name: "Posters" });
    expect(active.className).toContain("h-(--h-seg)");
    expect(active.className).toContain("bg-surface-active");
    expect(active.className).toContain("font-(weight:--fw-600)");
    expect(screen.getByRole("group", { name: "View" }).className).toContain("bg-surface-card");
  });
});

describe("Chip", () => {
  it("is a 24px pill that toggles and shows a count", async () => {
    const onClick = vi.fn();
    const { rerender } = render(
      <Chip active={false} count={12} onClick={onClick}>
        Unscraped
      </Chip>,
    );
    const chip = screen.getByRole("button", { name: "Unscraped12" });
    expect(chip).toHaveAttribute("aria-pressed", "false");
    expect(chip.className).toContain("h-(--h-chip)");
    expect(chip.className).toContain("rounded-(--r-pill)");
    expect(screen.getByText("12").className).toContain("font-(weight:--fw-600)");
    await userEvent.click(chip);
    expect(onClick).toHaveBeenCalledOnce();

    rerender(
      <Chip active count={12}>
        Unscraped
      </Chip>,
    );
    const on = screen.getByRole("button", { name: "Unscraped12" });
    expect(on).toHaveAttribute("aria-pressed", "true");
    expect(on.className).toContain("bg-gold-tint");
    expect(on.className).toContain("border-gold-tint-edge");
  });

  it("is inert when disabled", async () => {
    const onClick = vi.fn();
    render(
      <Chip disabled onClick={onClick}>
        Drive
      </Chip>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Drive" }));
    expect(onClick).not.toHaveBeenCalled();
  });
});

describe("Checkbox", () => {
  function Harness({ onChange }: { onChange?: (value: boolean) => void }) {
    const [checked, setChecked] = useState(false);
    return (
      <Checkbox
        aria-label="Select all"
        checked={checked}
        onCheckedChange={(value) => {
          setChecked(value);
          onChange?.(value);
        }}
      />
    );
  }

  it("toggles with Space and with a click", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const box = screen.getByRole("checkbox", { name: "Select all" });
    await userEvent.tab();
    expect(box).toHaveFocus();
    await userEvent.keyboard(" ");
    expect(box).toBeChecked();
    await userEvent.click(box);
    expect(box).not.toBeChecked();
    expect(onChange.mock.calls).toEqual([[true], [false]]);
  });

  it("is 14px, gold when on and reports a mixed state", () => {
    const { rerender } = render(
      <Checkbox aria-label="Some" checked={false} indeterminate onCheckedChange={() => {}} />,
    );
    const box = screen.getByRole("checkbox", { name: "Some" });
    expect(box).toBePartiallyChecked();
    expect(box.className).toContain("size-3.5");
    expect(box.className).toContain("bg-gold");
    rerender(<Checkbox aria-label="Some" checked onCheckedChange={() => {}} />);
    expect(screen.getByRole("checkbox")).not.toBePartiallyChecked();
    expect(screen.getByRole("checkbox")).toBeChecked();
  });

  it("is named by its label and inert when disabled", async () => {
    const onChange = vi.fn();
    render(
      <Checkbox checked={false} disabled onCheckedChange={onChange}>
        Delete files too
      </Checkbox>,
    );
    const box = screen.getByRole("checkbox", { name: "Delete files too" });
    expect(box).toBeDisabled();
    await userEvent.click(box);
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Switch", () => {
  it("toggles with Space and Enter and reports aria-checked", async () => {
    const onChange = vi.fn();
    function Harness() {
      const [on, setOn] = useState(true);
      return (
        <Switch
          aria-label="Install updates"
          checked={on}
          onCheckedChange={(value) => {
            setOn(value);
            onChange(value);
          }}
        />
      );
    }
    render(<Harness />);
    const toggle = screen.getByRole("switch", { name: "Install updates" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(toggle.firstElementChild?.className).toContain("bg-gold");
    await userEvent.tab();
    await userEvent.keyboard(" ");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle.firstElementChild?.className).toContain("bg-track");
    await userEvent.keyboard("{Enter}");
    expect(onChange.mock.calls).toEqual([[false], [true]]);
  });

  it("is inert when disabled", async () => {
    const onChange = vi.fn();
    render(<Switch aria-label="Off" checked={false} disabled onCheckedChange={onChange} />);
    await userEvent.click(screen.getByRole("switch"));
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Select", () => {
  const options = [
    { value: "all", label: "All drives" },
    { value: "c", label: "C:" },
  ];

  it("is a 30px field with a name that reports the chosen value", async () => {
    const onValueChange = vi.fn();
    render(
      <Select aria-label="Drive" options={options} value="all" onValueChange={onValueChange} />,
    );
    const select = screen.getByRole("combobox", { name: "Drive" });
    expect(select.parentElement?.className).toContain("h-(--h-field)");
    expect(select.parentElement?.className).toContain("bg-surface-card");
    await userEvent.selectOptions(select, "c");
    expect(onValueChange).toHaveBeenCalledWith("c");
  });

  it("takes the well colour on a floating surface and is inert when disabled", () => {
    render(
      <SurfaceContext value="floating">
        <Select
          aria-label="Drive"
          options={options}
          value="all"
          onValueChange={() => {}}
          disabled
        />
      </SurfaceContext>,
    );
    const select = screen.getByRole("combobox", { name: "Drive" });
    expect(select).toBeDisabled();
    expect(select.parentElement?.className).toContain("bg-surface-well");
  });
});
