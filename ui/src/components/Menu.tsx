import { Check } from "lucide-react";
import { ContextMenu, DropdownMenu } from "radix-ui";
import { useContext } from "react";
import type { ComponentProps, CSSProperties, ReactElement, ReactNode } from "react";

import { cx } from "../lib/cx";
import { menuItemClass, MenuKindContext, menuSurface, useFloatingContainer } from "./floating";
import { Icon } from "./Icon";
import { SurfaceContext } from "./surface";

interface MenuProps {
  /** The control that opens the menu (a Button or IconButton); it gets the menu's ARIA wiring. */
  trigger: ReactElement;
  /** The items: `MenuItem`, `MenuSeparator`, `MenuLabel`, `MenuRadioGroup`. */
  children: ReactNode;
  /** Names the menu for assistive technology when the trigger's text is not enough. */
  "aria-label"?: string;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Which edge of the trigger the menu lines up with. */
  align?: "start" | "center" | "end";
  side?: "top" | "right" | "bottom" | "left";
  /** Modal menus block the page while open (the default); a non-modal one leaves it usable. */
  modal?: boolean;
  /** Extra classes for the floating surface, for example a wider minimum. */
  className?: string;
  style?: CSSProperties;
  /** Overrides where focus goes when the menu opens (the menu, or its first item from the keyboard). */
  onOpenAutoFocus?: (event: Event) => void;
  /** Overrides where focus goes when the menu closes (it returns to the trigger by default). */
  onCloseAutoFocus?: (event: Event) => void;
}

/**
 * A drop-down menu on the floating surface: padding 4, radius 10,
 * at least 264 wide, 28px items with key hints, separators; no submenus). It fades in and grows
 * from its trigger (`.cv-pop`), arrow keys move through the items, typing jumps to one, Esc and an
 * outside click close it, and focus returns to the trigger.
 */
export function Menu({
  trigger,
  children,
  "aria-label": ariaLabel,
  open,
  defaultOpen,
  onOpenChange,
  align = "start",
  side = "bottom",
  modal,
  className,
  style,
  onOpenAutoFocus,
  onCloseAutoFocus,
}: MenuProps) {
  const container = useFloatingContainer();
  return (
    <DropdownMenu.Root
      open={open}
      defaultOpen={defaultOpen}
      onOpenChange={onOpenChange}
      modal={modal}
    >
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal container={container}>
        <DropdownMenu.Content
          // A name of its own replaces the trigger's text as the menu's name.
          {...(ariaLabel === undefined
            ? {}
            : { "aria-label": ariaLabel, "aria-labelledby": undefined })}
          align={align}
          side={side}
          sideOffset={4}
          collisionPadding={8}
          // Radix's menu content runs this handler but leaves it out of its public props.
          {...({ onOpenAutoFocus } as object)}
          onCloseAutoFocus={onCloseAutoFocus}
          className={cx(
            menuSurface,
            "max-h-(--radix-dropdown-menu-content-available-height)",
            className,
          )}
          style={style}
        >
          <MenuKindContext value="dropdown">
            <SurfaceContext value="floating">{children}</SurfaceContext>
          </MenuKindContext>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** A divider between groups of items: the section strength, 4px above and below, 6px in. */
export function MenuSeparator({ className }: { className?: string }) {
  const kind = useContext(MenuKindContext);
  const Separator = kind === "context" ? ContextMenu.Separator : DropdownMenu.Separator;
  return <Separator className={cx("mx-1.5 my-1 h-px bg-line-section", className)} />;
}

/** A small heading over a group of items, in the one section-label style. */
export function MenuLabel({ className, children }: { className?: string; children: ReactNode }) {
  const kind = useContext(MenuKindContext);
  const Label = kind === "context" ? ContextMenu.Label : DropdownMenu.Label;
  return (
    <Label className={cx("type-section-label flex h-7 items-center px-2.5", className)}>
      {children}
    </Label>
  );
}

type RadioGroupProps = Omit<ComponentProps<typeof DropdownMenu.RadioGroup>, "onValueChange"> & {
  value: string;
  onValueChange: (value: string) => void;
};

/** A group of items of which one is chosen (a sort order, a view). */
export function MenuRadioGroup(props: RadioGroupProps) {
  const kind = useContext(MenuKindContext);
  const Group = kind === "context" ? ContextMenu.RadioGroup : DropdownMenu.RadioGroup;
  return <Group {...props} />;
}

type RadioItemProps = ComponentProps<typeof DropdownMenu.RadioItem>;

/** One choice of a `MenuRadioGroup`: a check at the right marks the chosen one. */
export function MenuRadioItem({ className, children, ...rest }: RadioItemProps) {
  const kind = useContext(MenuKindContext);
  const Item = kind === "context" ? ContextMenu.RadioItem : DropdownMenu.RadioItem;
  const Indicator = kind === "context" ? ContextMenu.ItemIndicator : DropdownMenu.ItemIndicator;
  return (
    <Item
      {...rest}
      className={cx(
        menuItemClass,
        "data-[disabled]:cursor-not-allowed data-[disabled]:text-text-disabled",
        "data-[state=checked]:font-(weight:--fw-600)",
        className,
      )}
    >
      <span className="min-w-0 flex-1 truncate">{children}</span>
      <Indicator className="ml-auto inline-flex">
        <Icon icon={Check} />
      </Indicator>
    </Item>
  );
}

type CheckboxItemProps = ComponentProps<typeof DropdownMenu.CheckboxItem>;

/**
 * An on/off item: a check at the right shows it is on. It leaves the menu open when chosen (several
 * can be switched in a row, a column chooser); pass `onSelect` to change that.
 */
export function MenuCheckboxItem({ className, children, onSelect, ...rest }: CheckboxItemProps) {
  const kind = useContext(MenuKindContext);
  const Item = kind === "context" ? ContextMenu.CheckboxItem : DropdownMenu.CheckboxItem;
  const Indicator = kind === "context" ? ContextMenu.ItemIndicator : DropdownMenu.ItemIndicator;
  return (
    <Item
      {...rest}
      onSelect={onSelect ?? ((event) => event.preventDefault())}
      className={cx(
        menuItemClass,
        "data-[disabled]:cursor-not-allowed data-[disabled]:text-text-disabled",
        className,
      )}
    >
      <span className="min-w-0 flex-1 truncate">{children}</span>
      <Indicator className="ml-auto inline-flex">
        <Icon icon={Check} />
      </Indicator>
    </Item>
  );
}
