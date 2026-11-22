import type { TableStructure } from "../../api";
import { count } from "../../i18n";
import { tableGrid } from "./edits";

export default function TableEditor({
  table,
  onChange,
  rtl,
  active = false,
}: {
  table: TableStructure;
  onChange: (cellIndex: number, text: string) => void;
  rtl: boolean;
  active?: boolean;
}) {
  if (table.cells.length === 0) {
    return (
      <p className="px-1 py-2 text-xs text-zinc-500">
        {rtl ? "لم يتم التعرف على خلايا" : "No cells recognized"}
      </p>
    );
  }

  const grid = tableGrid(table);

  return (
    <div className="-mx-1 overflow-x-auto rounded-lg">
      <table className="w-full border-collapse text-xs">
        <caption className="sr-only">
          {rtl ? "جدول قابل للتحرير" : "Editable table"}
        </caption>
        <tbody>
          {grid.map((line, row) => (
            <tr key={row}>
              {line.map((slot, col) => {
                if (slot.kind === "covered") return null;
                if (slot.kind === "empty") {
                  return (
                    <td
                      key={col}
                      className="border border-white/[0.07] bg-white/[0.01] p-1"
                    />
                  );
                }
                const { cell, index } = slot;
                return (
                  <td
                    key={col}
                    rowSpan={cell.row_span > 1 ? cell.row_span : undefined}
                    colSpan={cell.col_span > 1 ? cell.col_span : undefined}
                    className={`border p-0 align-top transition-colors ${
                      active ? "border-accent-400/25" : "border-white/[0.09]"
                    }`}
                  >
                    <input
                      value={cell.text}
                      onChange={(event) => onChange(index, event.target.value)}
                      dir="auto"
                      style={{
                        width: `${Math.min(28, Math.max(5, cell.text.length + 3))}ch`,
                      }}
                      aria-label={`${rtl ? "صف" : "row"} ${cell.row + 1} ${rtl ? "عمود" : "column"} ${cell.col + 1}`}
                      className="w-full min-w-[3.5rem] bg-transparent px-2 py-1.5 text-[0.8rem] text-zinc-100 outline-none transition placeholder:text-zinc-600 focus:bg-accent-400/10"
                    />
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="mt-1.5 flex items-center gap-1.5 px-1 font-mono text-[0.6rem] text-zinc-600">
        <span dir="ltr">
          {table.rows} × {table.cols}
        </span>
        <span aria-hidden="true">·</span>
        <span>{count("cells", table.cells.length, rtl)}</span>
      </p>
    </div>
  );
}
