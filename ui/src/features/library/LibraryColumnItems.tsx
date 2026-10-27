import { MenuCheckboxItem } from "../../components/Menu";

export interface ColumnChoice {
  id: string;
  label: string;
  shown: boolean;
}

/**
 * The column chooser's items: a check for each column that is shown. The Columns button of the
 * toolbar and the right-click menu of the table header both open it. Several columns can be
 * switched in a row, so it stays open; the Title column is not in it, it is always shown.
 */
export function LibraryColumnItems({
  columns,
  onColumn,
}: {
  columns: readonly ColumnChoice[];
  onColumn: (id: string, shown: boolean) => void;
}) {
  return (
    <>
      {columns.map((column) => (
        <MenuCheckboxItem
          key={column.id}
          checked={column.shown}
          onCheckedChange={(checked) => onColumn(column.id, checked)}
        >
          {column.label}
        </MenuCheckboxItem>
      ))}
    </>
  );
}
